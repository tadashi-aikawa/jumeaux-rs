//! アドオンが実際に選んだ分岐の記録。条件式をログ用に再評価しない。
use crate::{models::CognitionDiffs, notification::Verbosity};

#[derive(Debug, Clone, PartialEq)]
pub enum AddonLog {
    Conversion {
        format: &'static str,
        reason: ConversionReason,
    },
    Stored {
        expression: String,
    },
    RegardAsSame {
        expression: String,
    },
    CognitionDiffs(CognitionDiffs),
    NoMatchingTags,
    Sleep {
        seconds: f64,
    },
    JsonSortSkipped,
    XmlParsed {
        success: bool,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionReason {
    AlreadyExists,
    Forced,
    ResponseType,
    DifferentType,
    Block,
}
impl AddonLog {
    pub fn level(&self) -> Verbosity {
        match self {
            Self::Stored { .. }
            | Self::RegardAsSame { .. }
            | Self::Sleep { .. }
            | Self::JsonSortSkipped => Verbosity::VeryVerbose,
            Self::XmlParsed { success: false } => Verbosity::Warning,
            _ => Verbosity::Debug,
        }
    }
}
pub(super) fn conversion(
    format: &'static str,
    force: bool,
    existed: bool,
    response_type: &str,
) -> AddonLog {
    let reason = if existed && !force {
        ConversionReason::AlreadyExists
    } else if force {
        ConversionReason::Forced
    } else if format == "block" {
        ConversionReason::Block
    } else if format == response_type {
        ConversionReason::ResponseType
    } else {
        ConversionReason::DifferentType
    };
    AddonLog::Conversion { format, reason }
}
