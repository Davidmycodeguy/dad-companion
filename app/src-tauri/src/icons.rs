//! The `icon` URI scheme: item icons straight from the icon pack
//! (`http://icon.localhost/icons/Weapon/ArmingSword_5001.webp` on Windows).

use std::borrow::Cow;

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext};

use crate::state::AppState;

pub const SCHEME: &str = "icon";

pub fn serve<R: Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = percent_encoding::percent_decode_str(request.uri().path()).decode_utf8_lossy();
    let bytes = ctx
        .app_handle()
        .try_state::<AppState>()
        .and_then(|state| state.icons.as_ref().and_then(|icons| icons.get(path.trim_start_matches('/'))));
    let response = Response::builder().header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    match bytes {
        Some(bytes) => response
            .header(header::CONTENT_TYPE, "image/webp")
            .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
            .body(Cow::Owned(bytes.as_ref().clone())),
        None => response.status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&[][..])),
    }
    .unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
}
