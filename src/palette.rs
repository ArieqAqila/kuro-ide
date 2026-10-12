//! Choosing colours that suit the terminal: *selection only*, never invention.
//!
//! kuro never writes RGB or 256-colour sequences. What it can do is look at the terminal's own colours (the 16 palette slots, the default
//! text colour and the background, asked with `OSC 4` / `OSC 10` / `OSC 11`) and choose, for roles nobody mapped, the palette slot that
//! stands out best; when no slot is distinct enough it adds a text style (bold, underline, italic) instead of a look-alike colour.
//! Everything here is pure, so it is tested without a terminal.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// The colours of a terminal, as far as it told us.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Palette {
    pub slots: [Option<Rgb>; 16],
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
}

/// Text styles used when colour alone cannot tell two roles apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    None,
    Bold,
    Underline,
    Italic,
    BoldUnderline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// A palette slot 0-15, or -1 for the default text colour.
    pub slot: i8,
    pub emphasis: Emphasis,
}

/// How much lighter or darker (CIE L*, 0-100) a role must be than the background to be legible, and how different (L*a*b*, ΔE76) it
/// must be from the default text and from the other roles.
pub const MIN_FROM_BACKGROUND: f64 = 30.0;
pub const MIN_FROM_TEXT: f64 = 15.0;
pub const MIN_BETWEEN_ROLES: f64 = 25.0;

fn lab(c: Rgb) -> [f64; 3] {
    let lin = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(c.0), lin(c.1), lin(c.2));
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883;
    let f = |t: f64| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The difference in lightness (L*): what makes text readable on a background, however different the hues are.
pub fn lightness_gap(a: Rgb, b: Rgb) -> f64 {
    (lab(a)[0] - lab(b)[0]).abs()
}

pub fn distance(a: Rgb, b: Rgb) -> f64 {
    let (p, q) = (lab(a), lab(b));
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

/// Parse what a terminal answers: `rgb:RRRR/GGGG/BBBB` (1-4 hex digits per channel) or `#rrggbb`.
pub fn parse_color(s: &str) -> Option<Rgb> {
    let s = s.trim();
    let channel = |h: &str| -> Option<u8> {
        if h.is_empty() || h.len() > 4 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        let max = (1u32 << (4 * h.len())) - 1;
        Some(((v * 255 + max / 2) / max) as u8)
    };
    if let Some(rest) = s.strip_prefix("rgb:") {
        let mut it = rest.split('/');
        let (r, g, b) = (channel(it.next()?)?, channel(it.next()?)?, channel(it.next()?)?);
        return it.next().is_none().then_some(Rgb(r, g, b));
    }
    let hex = s.strip_prefix('#')?;
    (hex.len() == 6).then(|| Some(Rgb(channel(&hex[0..2])?, channel(&hex[2..4])?, channel(&hex[4..6])?))).flatten()
}

impl Palette {
    /// Feed one `OSC` payload (`4;N;rgb:…`, `10;rgb:…` or `11;rgb:…`).
    pub fn feed_osc(&mut self, payload: &str) {
        let mut it = payload.splitn(3, ';');
        match (it.next(), it.next(), it.next()) {
            (Some("4"), Some(n), Some(c)) => {
                if let (Ok(n), Some(c)) = (n.parse::<usize>(), parse_color(c))
                    && n < 16
                {
                    self.slots[n] = Some(c);
                }
            }
            (Some("10"), Some(c), None) => self.fg = parse_color(c),
            (Some("11"), Some(c), None) => self.bg = parse_color(c),
            _ => {}
        }
    }

    /// Enough to choose from: the background and at least a few slots are known.
    pub fn usable(&self) -> bool {
        self.bg.is_some() && self.slots.iter().flatten().count() >= 8
    }
}

/// A role to place: its default slot, and whether the user pinned it (then it is never changed).
#[derive(Debug, Clone, Copy)]
pub struct RoleSpec {
    pub default_slot: i8,
    pub pinned: bool,
}

const EMPHASES: [Emphasis; 4] = [Emphasis::Bold, Emphasis::Underline, Emphasis::Italic, Emphasis::BoldUnderline];

/// Place every role. Pinned roles keep their slot; the others keep their default slot when it is distinct enough from the background,
/// the default text and the roles placed before them, and otherwise get the best slot, or a style on a good slot.
/// Without a usable palette every role simply keeps its default.
pub fn pick(palette: &Palette, roles: &[RoleSpec]) -> Vec<Choice> {
    let keep = |r: &RoleSpec| Choice { slot: r.default_slot, emphasis: Emphasis::None };
    if !palette.usable() {
        return roles.iter().map(keep).collect();
    }
    let bg = palette.bg.unwrap();
    let colour_of = |slot: i8| -> Option<Rgb> { if slot < 0 { palette.fg } else { palette.slots.get(slot as usize).copied().flatten() } };
    let mut placed: Vec<(Choice, Option<Rgb>)> = Vec::new();
    let mut out = Vec::new();
    // pinned roles first, so the others make room for them
    let pinned: Vec<(usize, Choice)> = roles.iter().enumerate().filter(|(_, r)| r.pinned).map(|(i, r)| (i, keep(r))).collect();
    for (_, c) in &pinned {
        placed.push((*c, colour_of(c.slot)));
    }
    for (i, r) in roles.iter().enumerate() {
        if let Some((_, c)) = pinned.iter().find(|(j, _)| *j == i) {
            out.push(*c);
            continue;
        }
        let fits = |slot: i8, taken: &[(Choice, Option<Rgb>)]| -> Option<f64> {
            let c = colour_of(slot)?;
            if lightness_gap(c, bg) < MIN_FROM_BACKGROUND || palette.fg.is_some_and(|f| distance(c, f) < MIN_FROM_TEXT) {
                return None;
            }
            Some(
                taken
                    .iter()
                    .filter(|(t, _)| t.emphasis == Emphasis::None)
                    .filter_map(|(_, tc)| *tc)
                    .map(|tc| distance(c, tc))
                    .fold(f64::INFINITY, f64::min),
            )
        };
        let choice = match fits(r.default_slot, &placed) {
            // the default is fine: keep it (stable look, and "dynamic" only when it is not)
            Some(d) if d >= MIN_BETWEEN_ROLES => keep(r),
            _ => {
                let best = (1..16i8).filter_map(|s| fits(s, &placed).map(|d| (s, d))).max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                match best {
                    Some((slot, d)) if d >= MIN_BETWEEN_ROLES => Choice { slot, emphasis: Emphasis::None },
                    // no slot is distinct enough from the others: a style on the best-standing slot
                    Some((slot, _)) => {
                        let near: Vec<Emphasis> = placed
                            .iter()
                            .filter(|(t, tc)| {
                                colour_of(slot).zip(*tc).is_some_and(|(a, b)| distance(a, b) < MIN_BETWEEN_ROLES) && t.slot >= -1
                            })
                            .map(|(t, _)| t.emphasis)
                            .collect();
                        let emphasis = EMPHASES.iter().copied().find(|e| !near.contains(e)).unwrap_or(Emphasis::BoldUnderline);
                        Choice { slot, emphasis }
                    }
                    // nothing stands out against this background at all: the default text colour with a style
                    None => {
                        let used: Vec<Emphasis> = placed.iter().filter(|(t, _)| t.slot == -1).map(|(t, _)| t.emphasis).collect();
                        Choice {
                            slot: -1,
                            emphasis: EMPHASES.iter().copied().find(|e| !used.contains(e)).unwrap_or(Emphasis::BoldUnderline),
                        }
                    }
                }
            }
        };
        placed.push((choice, colour_of(choice.slot)));
        out.push(choice);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classic xterm palette on black.
    fn dark() -> Palette {
        let c = [
            (0, 0, 0),
            (205, 0, 0),
            (0, 205, 0),
            (205, 205, 0),
            (0, 0, 238),
            (205, 0, 205),
            (0, 205, 205),
            (229, 229, 229),
            (127, 127, 127),
            (255, 0, 0),
            (0, 255, 0),
            (255, 255, 0),
            (92, 92, 255),
            (255, 0, 255),
            (0, 255, 255),
            (255, 255, 255),
        ];
        let mut p = Palette { fg: Some(Rgb(229, 229, 229)), bg: Some(Rgb(0, 0, 0)), ..Default::default() };
        for (i, (r, g, b)) in c.iter().enumerate() {
            p.slots[i] = Some(Rgb(*r, *g, *b));
        }
        p
    }

    fn light() -> Palette {
        let mut p = dark();
        p.bg = Some(Rgb(255, 255, 255));
        p.fg = Some(Rgb(30, 30, 30));
        p
    }

    fn four() -> Vec<RoleSpec> {
        [14, 11, 10, 13].iter().map(|s| RoleSpec { default_slot: *s, pinned: false }).collect()
    }

    #[test]
    fn colours_parse_in_the_forms_terminals_use() {
        assert_eq!(parse_color("rgb:ffff/0000/8080"), Some(Rgb(255, 0, 128)));
        assert_eq!(parse_color("rgb:ff/00/80"), Some(Rgb(255, 0, 128)));
        assert_eq!(parse_color("rgb:f/0/8"), Some(Rgb(255, 0, 136)));
        assert_eq!(parse_color("#102030"), Some(Rgb(16, 32, 48)));
        for bad in ["", "rgb:", "rgb:ff/00", "rgb:ff/00/80/00", "rgb:gg/00/00", "rgb:fffff/00/00", "#12345", "red", "rgba:1/2/3/4"] {
            assert_eq!(parse_color(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn osc_replies_fill_the_palette() {
        let mut p = Palette::default();
        p.feed_osc("4;1;rgb:cdcd/0000/0000");
        p.feed_osc("4;15;rgb:ffff/ffff/ffff");
        p.feed_osc("10;rgb:e5e5/e5e5/e5e5");
        p.feed_osc("11;rgb:0000/0000/0000");
        p.feed_osc("4;99;rgb:0000/0000/0000");
        p.feed_osc("garbage");
        p.feed_osc("4;x;rgb:0000/0000/0000");
        assert_eq!(
            (p.slots[1], p.slots[15], p.fg, p.bg),
            (Some(Rgb(205, 0, 0)), Some(Rgb(255, 255, 255)), Some(Rgb(229, 229, 229)), Some(Rgb(0, 0, 0)))
        );
        assert!(!p.usable(), "two slots are not enough");
        assert!(dark().usable());
    }

    #[test]
    fn distance_is_a_metric_and_orders_colours_sensibly() {
        let (w, b, y) = (Rgb(255, 255, 255), Rgb(0, 0, 0), Rgb(255, 255, 0));
        assert_eq!(distance(w, w), 0.0);
        assert!((distance(w, b) - distance(b, w)).abs() < 1e-9);
        assert!(distance(w, b) > 99.0, "white and black are 100 apart in L*");
        assert!(lightness_gap(y, w) < MIN_FROM_BACKGROUND, "bright yellow is a poor choice on a white background");
        assert!(lightness_gap(y, b) > 90.0, "and a fine one on black");
    }

    #[test]
    fn on_a_good_dark_theme_the_declared_slots_are_kept() {
        let got = pick(&dark(), &four());
        assert_eq!(got.iter().map(|c| c.slot).collect::<Vec<_>>(), vec![14, 11, 10, 13]);
        assert!(got.iter().all(|c| c.emphasis == Emphasis::None));
    }

    #[test]
    fn on_a_light_theme_the_pale_slots_are_replaced_by_ones_that_stand_out() {
        let p = light();
        let got = pick(&p, &four());
        for (c, want_default) in got.iter().zip([14, 11, 10, 13]) {
            let colour = p.slots[c.slot as usize].unwrap();
            assert!(lightness_gap(colour, p.bg.unwrap()) >= MIN_FROM_BACKGROUND, "{c:?} is legible on white");
            let _ = want_default;
        }
        assert_ne!(got[1].slot, 11, "bright yellow on white is replaced");
        // and the four still differ from each other
        for i in 0..4 {
            for j in i + 1..4 {
                let (a, b) = (got[i], got[j]);
                assert!(a.slot != b.slot || a.emphasis != b.emphasis, "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn pinned_roles_never_change_and_others_avoid_them() {
        let mut roles = four();
        roles[0] = RoleSpec { default_slot: 11, pinned: true };
        let got = pick(&light(), &roles);
        assert_eq!(got[0], Choice { slot: 11, emphasis: Emphasis::None }, "pinned: kept even where it is a poor choice");
        for c in &got[1..] {
            assert_ne!(c.slot, 11);
        }
    }

    #[test]
    fn a_poor_palette_falls_back_to_styles_not_look_alike_colours() {
        // a three-colour terminal: black, white and one red; the rest are copies of black or white
        let mut p = Palette { bg: Some(Rgb(0, 0, 0)), fg: Some(Rgb(255, 255, 255)), ..Default::default() };
        for i in 0..16 {
            p.slots[i] = Some(if i == 1 {
                Rgb(255, 0, 0)
            } else if i < 8 {
                Rgb(0, 0, 0)
            } else {
                Rgb(255, 255, 255)
            });
        }
        let got = pick(&p, &four());
        let distinct: std::collections::HashSet<String> = got.iter().map(|c| format!("{c:?}")).collect();
        assert_eq!(distinct.len(), 4, "four roles stay distinguishable: {got:?}");
        assert!(got.iter().any(|c| c.emphasis != Emphasis::None), "some of them use a style: {got:?}");
        for c in &got {
            assert!((-1..16).contains(&c.slot), "only palette slots or the default text, never an invented colour");
        }
    }

    #[test]
    fn nothing_known_means_the_defaults() {
        let got = pick(&Palette::default(), &four());
        assert_eq!(got.iter().map(|c| c.slot).collect::<Vec<_>>(), vec![14, 11, 10, 13]);
        assert!(pick(&dark(), &[]).is_empty());
    }

    #[test]
    fn the_choice_is_deterministic() {
        assert_eq!(pick(&light(), &four()), pick(&light(), &four()));
    }
}
