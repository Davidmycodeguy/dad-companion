//! Plain structs mirroring exactly the fields `quest_packet_handler.py` reads from each decoded
//! merchant/quest message. This crate never depends on the `protocol` crate's generated protobuf
//! types (built in parallel by another agent): the app converts a real decoded message into these
//! with a few lines once that crate is ready. Field names follow the proto's own naming
//! (`protos/Merchant.proto`) translated to Rust's snake_case convention; doc comments name the proto
//! message each struct mirrors, and call out any field the Python handler never reads (so it has no
//! equivalent here).

/// The `SMERCHANT_QUEST_INFO.FLAG` enum: what state a quest is in for its merchant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestFlag {
    /// Not yet accepted.
    None,
    /// Accepted, objectives in progress.
    Progress,
    /// All objectives met, ready to turn in.
    Success,
    /// Already turned in.
    Complete,
    /// Not available yet (earlier chapter unfinished).
    Locked,
    /// Available to accept.
    Available,
    /// A value the current game version doesn't map to a known flag.
    Unknown(i32),
}

impl QuestFlag {
    pub fn from_raw(value: i32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Progress,
            2 => Self::Success,
            3 => Self::Complete,
            4 => Self::Locked,
            5 => Self::Available,
            other => Self::Unknown(other),
        }
    }

    /// The label `quest_packet_handler.QUEST_FLAG_LABELS` used for this flag.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Progress => "progress",
            Self::Success => "success",
            Self::Complete => "complete",
            Self::Locked => "locked",
            Self::Available => "available",
            Self::Unknown(_) => "unknown",
        }
    }

    /// The wire value this flag round-trips to.
    pub fn to_raw(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Progress => 1,
            Self::Success => 2,
            Self::Complete => 3,
            Self::Locked => 4,
            Self::Available => 5,
            Self::Unknown(value) => value,
        }
    }
}

/// The `SMERCHANT_INFO.FLAG` enum: what a merchant currently offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MerchantFlag {
    None,
    /// Has a quest ready to accept or in progress.
    Quest,
    /// Has a quest ready to turn in.
    Success,
    Recovery,
    Express,
    Notify,
    Parcel,
    Unknown(u32),
}

impl MerchantFlag {
    pub fn from_raw(value: u32) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Quest,
            2 => Self::Success,
            3 => Self::Recovery,
            4 => Self::Express,
            5 => Self::Notify,
            6 => Self::Parcel,
            other => Self::Unknown(other),
        }
    }
}

/// `SMERCHANT_INFO`, as read by `handle_merchant_list` (only `merchantId`/`merchantFlag`; the proto
/// also carries `remainTime`, `isUnidentified`, `affinity` and `affinityId`, none of which the
/// Python handler reads).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MerchantInfoInput {
    pub merchant_id: String,
    pub merchant_flag: u32,
}

/// `SS2C_MERCHANT_LIST_RES`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MerchantListMessage {
    pub merchant_list: Vec<MerchantInfoInput>,
}

/// `SMERCHANT_QUEST_CONTENT_INFO`: one submitted-item counter inside a quest's `missions` list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestContentInfoInput {
    pub content_id: String,
    pub content_current_value: i32,
}

/// `SMERCHANT_QUEST_CHAPTER_INFO`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestChapterInfoInput {
    pub chapter_id: String,
    pub remain_ms_time: u64,
}

/// `SMERCHANT_QUEST_INFO`, as read by `_parse_quest_info` (the proto's `requiredQuestMerchantId` is
/// never read by the Python handler, so it has no field here).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestInfoInput {
    pub quest_order: u32,
    pub quest_id: String,
    pub chapter_id: String,
    pub quest_flag: i32,
    pub already_get_affinity: i32,
    pub missions: Vec<QuestContentInfoInput>,
}

/// `SS2C_MERCHANT_QUEST_LIST_INFO_RES` (its `result` field is never read by `handle_quest_list`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestListMessage {
    pub quests: Vec<QuestInfoInput>,
    pub chapters: Vec<QuestChapterInfoInput>,
}

/// `SMERCHANT_QUEST_LOG_INFO`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestLogEntryInput {
    pub merchant_id: String,
    pub quests: Vec<QuestInfoInput>,
    pub chapters: Vec<QuestChapterInfoInput>,
}

/// `SS2C_MERCHANT_QUEST_LOG_LIST_RES` (its `result` field is never read by `handle_quest_log`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestLogMessage {
    pub quest_list: Vec<QuestLogEntryInput>,
}

/// `SS2C_MERCHANT_QUEST_SELECT_RES`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestSelectMessage {
    pub result: i32,
}

/// `SREWARD_INFO` (defined in `protos/Shop.proto`, used by the quest-complete response).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RewardInfoInput {
    pub reward_type: String,
    pub stock_id: String,
    pub reward_count: u32,
}

/// `SS2C_MERCHANT_QUEST_COMPLETE_RES`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestCompleteMessage {
    pub result: i32,
    pub given_merchant_id: String,
    pub given_quest_id: String,
    pub given_chapter_id: String,
    pub rewards: Vec<RewardInfoInput>,
}

/// `SS2C_MERCHANT_QUEST_CONTENT_VALUE_STACK_RES`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestContentValueStackMessage {
    pub result: i32,
}
