use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

impl std::str::FromStr for Lang {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "en" | "english" => Ok(Lang::En),
            "zh" | "chinese" | "cn" => Ok(Lang::Zh),
            _ => Err(format!("unknown language: {} (use en or zh)", s)),
        }
    }
}

static LANG: OnceLock<Lang> = OnceLock::new();

pub fn set_lang(lang: Lang) {
    LANG.set(lang).ok();
}

pub fn lang() -> Lang {
    *LANG.get().unwrap_or(&Lang::En)
}

/// Bilingual string selector: t!("english", "中文")
macro_rules! t {
    ($en:expr, $zh:expr) => {
        match $crate::lang::lang() {
            $crate::lang::Lang::En => $en,
            $crate::lang::Lang::Zh => $zh,
        }
    };
}
pub(crate) use t;
