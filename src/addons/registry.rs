//! 名前・設定型・生成に使う唯一の登録表。
use super::{Layer, validate_registration};
use crate::{
    compat::Result,
    config::{deserialize, invalid},
    models::Addon,
};
use serde_json::Value;

macro_rules! register {
    ($($layer:ident, $module:ident { $($variant:ident: $name:literal => $config:ident),+ $(,)? });+ $(;)?) => {
        #[derive(Debug, Clone, serde::Serialize)]
        #[serde(untagged)]
        pub enum AddonConfig { $($( $variant(super::$module::$config::Config), )+)+ }

        impl Layer {
            pub fn names(self) -> &'static [&'static str] {
                match self { $( Self::$layer => &[$($name),+], )+ }
            }
        }

        pub fn parse_config(layer: Layer, addon: &Addon, path: &str) -> Result<AddonConfig> {
            validate_registration(layer, addon, path)?;
            let value = Value::Object(addon.config.clone().unwrap_or_default());
            let path = format!("{path}.config");
            let config = match (layer, addon.name.as_str()) {
                $($( (Layer::$layer, $name) => AddonConfig::$variant(deserialize(&value, &path)?), )+)+
                _ => return Err(invalid(&path, "unsupported addon")),
            };
            if matches!(&config, AddonConfig::Res2ResJson(_)) {
                return Err(invalid(&format!("{path}.transformer"),
                    "Python transformer is unsupported; external command contract is undecided"));
            }
            if let AddonConfig::FinalMiroir(config) = &config {
                config.validate()?;
            }
            Ok(config)
        }
    };
}

register! {
    Log2Reqs, log2reqs {
        Log2ReqsPlain: "plain" => plain,
        Log2ReqsCsv: "csv" => csv,
        Log2ReqsJson: "json" => json,
        Log2ReqsYaml: "yaml" => yaml
    };
    Reqs2Reqs, reqs2reqs {
        Reqs2ReqsAdd: "add" => add,
        Reqs2ReqsEmptyGuard: "empty_guard" => empty_guard,
        Reqs2ReqsFilter: "filter" => filter,
        Reqs2ReqsHead: "head" => head,
        Reqs2ReqsRepeat: "repeat" => repeat,
        Reqs2ReqsRename: "rename" => rename,
        Reqs2ReqsReplace: "replace" => replace,
        Reqs2ReqsShuffle: "shuffle" => shuffle
    };
    Res2Res, res2res {
        Res2ResJson: "json" => json,
        Res2ResJsonSort: "json_sort" => json_sort,
        Res2ResType: "type" => r#type
    };
    Res2Dict, res2dict {
        Res2DictHtml: "html" => html,
        Res2DictJson: "json" => json,
        Res2DictXml: "xml" => xml,
        Res2DictBlock: "block" => block
    };
    Judgement, judgement {
        JudgementSame: "same" => same,
        JudgementIgnore: "ignore" => ignore
    };
    StoreCriterion, store_criterion {
        StoreCriterionFree: "free" => free
    };
    Dump, dump {
        DumpEncoding: "encoding" => encoding,
        DumpHtml: "html" => html,
        DumpJson: "json" => json,
        DumpXml: "xml" => xml
    };
    DidChallenge, did_challenge {
        DidChallengeSleep: "sleep" => sleep,
        DidChallengeTag: "tag" => tag
    };
    Final, r#final {
        FinalCsv: "csv" => csv,
        FinalJson: "json" => json,
        FinalSummary: "summary" => summary,
        FinalNotify: "notify" => notify,
        FinalViewer: "viewer" => viewer,
        FinalMiroir: "miroir" => miroir
    }
}

pub fn validate_addon(layer: Layer, addon: &Addon, path: &str) -> Result<()> {
    parse_config(layer, addon, path).map(|_| ())
}
