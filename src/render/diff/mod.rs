pub(super) const HUNK_HEADER_MINUS: char = '−';

pub(super) use super::{CONTENT_HORIZONTAL_PADDING, SCROLLBAR_WIDTH};

mod body;
mod builder_common;
mod builder_split;
mod builder_split_body;
mod builder_unified;
mod frame;
mod frame_close;
mod gutter;
mod layout;
mod preview;
mod truncate;

pub(crate) use body::{display_text, resolve_file_syntax, syntect_to_color};
#[cfg(test)]
pub(crate) use builder_split::build_split_lines;
pub(crate) use builder_split::{build_split_lines_with, BuiltSplitLines};
#[cfg(test)]
pub(crate) use builder_unified::build_diff_lines;
pub(crate) use builder_unified::{build_diff_lines_with, count_file_changes, BuiltDiffLines};
pub(crate) use frame::mode_change_label;
pub(crate) use frame_close::{close_frame_right, compute_sticky_top_idx, FrameDecorations};
pub(crate) use gutter::apply_number_gutter;
pub(crate) use layout::render_diff_panel;
pub(crate) use preview::render_diff_preview_panel;
pub(crate) use truncate::{truncate_spans_tail, truncate_unified_body_row};

use body::*;
use builder_split::*;
use builder_split_body::*;
use builder_unified::*;
use frame::*;
use frame_close::*;
use gutter::*;
use truncate::*;
