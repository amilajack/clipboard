//! Tab completion scripts, printed by `cb completions SHELL`.
//!
//! The scripts live in `completions/` so packages can install them as they are.

/// The shells there are scripts for, as named in errors.
pub const SHELLS: &str = "bash, zsh or fish";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }

    pub fn script(self) -> &'static str {
        match self {
            Self::Bash => include_str!("../completions/cb.bash"),
            Self::Zsh => include_str!("../completions/_cb"),
            Self::Fish => include_str!("../completions/cb.fish"),
        }
    }
}
