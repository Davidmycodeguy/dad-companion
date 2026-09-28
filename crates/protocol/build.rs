//! Compiles the game's `.proto` files into Rust types, and builds a lookup
//! table from `PacketCommand` enum values to the message type name each one
//! maps to.
//!
//! `protoc` is not installed in this environment. `protox` (a pure-Rust
//! protobuf compiler) parses the `.proto` files into a `FileDescriptorSet`
//! directly, which `prost-build` then turns into Rust types via
//! `Config::compile_fds` instead of shelling out to a `protoc` binary.
//!
//! The packet-type lookup table mirrors `_build_proto_map` in the Python
//! reference (`UI/src/models/capture.py`): for every `PacketCommand` value,
//! try the message named `"S" + value_name`, then `value_name` itself, and
//! record whichever one actually exists. Doing this once here, from the
//! descriptor set, avoids hand-maintaining a match arm per packet type.

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use prost::Message as _;
use prost_types::FileDescriptorSet;

/// `PacketCommand` value-name prefixes that are range markers, not real
/// packet types. Mirrors `capture.py`'s `_build_proto_map` `skipped_prefixes`.
const SKIPPED_PREFIXES: [&str; 3] = ["MIN_", "MAX_", "PACKET_NONE"];

/// Name of the enum (in `_PacketCommand.proto`) that lists every packet type.
const PACKET_COMMAND_ENUM: &str = "PacketCommand";

fn main() {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let protos_dir = manifest_dir.join("../../protos");
    let proto_files = discover_proto_files(&protos_dir);
    assert!(
        !proto_files.is_empty(),
        "no .proto files found under {}",
        protos_dir.display()
    );

    println!("cargo:rerun-if-changed={}", protos_dir.display());
    for file in &proto_files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let file_descriptor_set = protox::compile(&proto_files, [&protos_dir])
        .expect("failed to compile .proto files with protox");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    // Descriptor set and name map first, from the *true* proto names, before
    // the Rust-codegen-only rename below touches anything.
    save_descriptor_set(&file_descriptor_set, &out_dir);
    generate_packet_message_map(&file_descriptor_set, &out_dir);

    let mut file_descriptor_set = file_descriptor_set;
    rename_duplicate_message_name(&mut file_descriptor_set);

    prost_build::Config::new()
        .compile_fds(file_descriptor_set)
        .expect("failed to generate Rust types from the compiled descriptors");
}

/// `Operate.proto` defines both `Operate_character_info` and
/// `Operate_characterInfo` -- two distinct messages that prost-build's
/// UpperCamelCase conversion both turn into the Rust identifier
/// `OperateCharacterInfo`, which fails the build with "conflicting
/// implementations" (confirmed to be the *only* such collision among the
/// ~600 generated types). Renaming the second one here, in the descriptor
/// set, resolves it: it is a standalone message with no other message's
/// field referencing it (unlike `Operate_character_info`, which
/// `Operate_character_info_list` embeds), so no cross-reference needs
/// fixing up to match. Neither message is part of the game's own lobby
/// protocol used by [`crate::messages`] (both belong to an internal
/// operator/admin API in the same file), so the rename has no effect on
/// this crate's behavior.
const DUPLICATE_MESSAGE_NAME: &str = "Operate_characterInfo";
const DUPLICATE_MESSAGE_RENAMED: &str = "Operate_characterInfo_Full";

fn rename_duplicate_message_name(file_descriptor_set: &mut FileDescriptorSet) {
    let duplicate = file_descriptor_set
        .file
        .iter_mut()
        .flat_map(|file| file.message_type.iter_mut())
        .find(|message| message.name.as_deref() == Some(DUPLICATE_MESSAGE_NAME));
    if let Some(duplicate) = duplicate {
        duplicate.name = Some(DUPLICATE_MESSAGE_RENAMED.to_string());
    }
    // If the proto files changed and this message is gone, there is nothing
    // to rename. If they instead introduced a *different* collision,
    // `compile_fds` below fails loudly with a "conflicting implementations"
    // error naming the culprit, rather than silently miscompiling.
}

/// Every `*.proto` file directly under `dir` (imports are resolved by protox
/// via the include path; the `google/` well-known types are unused by these
/// protos and are not imported, so they do not need to be listed as roots).
fn discover_proto_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read protos dir {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "proto"))
        .collect();
    files.sort();
    files
}

/// Persists the raw descriptor set so a future consumer can do name-based
/// reflection (e.g. via `prost-reflect`) without re-running protox.
/// Exposed at runtime as `messages::FILE_DESCRIPTOR_SET_BYTES`.
fn save_descriptor_set(file_descriptor_set: &FileDescriptorSet, out_dir: &Path) {
    let bytes = file_descriptor_set.encode_to_vec();
    fs::write(out_dir.join("file_descriptor_set.bin"), bytes)
        .expect("failed to write file_descriptor_set.bin");
}

/// Builds `resolve_message_name` (`PacketCommand` value -> mapped message
/// name) and writes it to `OUT_DIR/packet_message_map.rs`, included from
/// `src/messages.rs`.
fn generate_packet_message_map(file_descriptor_set: &FileDescriptorSet, out_dir: &Path) {
    let message_names: HashSet<&str> = file_descriptor_set
        .file
        .iter()
        .flat_map(|file| file.message_type.iter())
        .filter_map(|message| message.name.as_deref())
        .collect();

    let packet_command = file_descriptor_set
        .file
        .iter()
        .flat_map(|file| file.enum_type.iter())
        .find(|candidate| candidate.name.as_deref() == Some(PACKET_COMMAND_ENUM))
        .unwrap_or_else(|| panic!("{PACKET_COMMAND_ENUM} enum not found in compiled descriptors"));

    // BTreeMap: proto3 enum values are normally unique, sorted output is
    // stable across builds, and a stray duplicate silently keeps one entry
    // instead of failing the build with an "unreachable pattern" error.
    let mut mapped: BTreeMap<i32, &str> = BTreeMap::new();
    for value in &packet_command.value {
        let (Some(name), Some(number)) = (value.name.as_deref(), value.number) else {
            continue;
        };
        if SKIPPED_PREFIXES.iter().any(|prefix| name.starts_with(*prefix)) {
            continue;
        }

        // Naming convention from `_build_proto_map`: try "S" + name first,
        // then the bare enum value name, and skip values with neither.
        let prefixed = format!("S{name}");
        let found = message_names
            .get(prefixed.as_str())
            .or_else(|| message_names.get(name))
            .copied();
        if let Some(found) = found {
            mapped.insert(number, found);
        }
    }

    let mut source = String::new();
    source.push_str("// @generated by build.rs from the compiled .proto descriptors.\n");
    source.push_str("// PacketCommand value -> the message type name mapped to it, mirroring\n");
    source.push_str("// `_build_proto_map` in the Python reference's `capture.py`.\n\n");
    source.push_str(&format!(
        "pub(crate) const MAPPED_PACKET_COUNT: usize = {};\n\n",
        mapped.len()
    ));
    source.push_str("pub(crate) fn resolve_message_name(packet_type: i32) -> Option<&'static str> {\n");
    source.push_str("    match packet_type {\n");
    for (number, name) in &mapped {
        source.push_str(&format!("        {number} => Some(\"{name}\"),\n"));
    }
    source.push_str("        _ => None,\n");
    source.push_str("    }\n");
    source.push_str("}\n");

    fs::write(out_dir.join("packet_message_map.rs"), source)
        .expect("failed to write packet_message_map.rs");
}
