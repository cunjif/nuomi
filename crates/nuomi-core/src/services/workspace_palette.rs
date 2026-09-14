//! Workspace color palette: deterministic path-hash → hand-drawn color tag.
//!
//! Each workspace gets a stable color derived from its root path via FNV-1a
//! hashing into an 8-color hand-drawn paper palette. No user selection needed.

/// A color tag identifying one entry in the hand-drawn palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorTag(pub &'static str);

/// The 8-color hand-drawn paper palette (纸黄/墨蓝/苔绿/砖红/紫罗兰/橙赭/青灰/粉桃).
/// Keys match the i18n entries `workspace.colorTag.<key>`.
pub const PALETTE: &[ColorTag] = &[
    ColorTag("paper-yellow"),
    ColorTag("ink-blue"),
    ColorTag("moss-green"),
    ColorTag("brick-red"),
    ColorTag("violet"),
    ColorTag("ochre-orange"),
    ColorTag("slate-gray"),
    ColorTag("peach-pink"),
];

/// FNV-1a 32-bit hash of the path bytes (deterministic, platform-independent).
pub fn hash(path: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5; // FNV offset basis
    for byte in path.as_bytes() {
        h ^= *byte as u32;
        h = h.wrapping_mul(0x0100_0193); // FNV prime
    }
    h
}

/// Returns the palette color for `path`. Same path always yields the same
/// color; empty path is safe (returns a deterministic palette entry).
pub fn color_for(path: &str) -> ColorTag {
    let idx = (hash(path) as usize) % PALETTE.len();
    PALETTE[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_path_same_color_deterministic() {
        for path in ["C:\\projects\\nuomi", "/home/james/code", "D:\\ws\\demo"] {
            let c1 = color_for(path);
            let c2 = color_for(path);
            assert_eq!(c1, c2, "path {path} must map to a stable color");
        }
    }

    #[test]
    fn empty_path_does_not_panic() {
        let c = color_for("");
        assert!(PALETTE.contains(&c));
    }

    #[test]
    fn palette_has_eight_hand_drawn_colors() {
        assert_eq!(PALETTE.len(), 8);
        // All keys are non-empty static strings.
        for ColorTag(key) in PALETTE {
            assert!(!key.is_empty());
        }
    }

    #[test]
    fn hash_fnv1a_known_value() {
        // FNV-1a of empty string is the offset basis.
        assert_eq!(hash(""), 0x811c_9dc5);
    }

    #[test]
    fn diverse_paths_cover_multiple_colors() {
        use std::collections::HashSet;
        let colors: HashSet<_> = (0..200)
            .map(|i| color_for(&format!("C:\\ws\\project_{i}")))
            .collect();
        // With 200 distinct paths and 8 colors, we expect broad coverage
        // (at least 6 of 8 — allowing minor clustering).
        assert!(
            colors.len() >= 6,
            "expected broad palette coverage, got {colors:?}"
        );
    }
}
