//! tokens.rs -- généré par gen/gen_tokens.py (cdx-ui 2.0.1) depuis tokens.json : ne pas modifier à la main.
//! Couleurs au format 0xRRGGBB.
#![allow(dead_code)]

// Couleurs de statut : les mêmes dans tous les thèmes.
pub const GREEN: u32 = 0x00FF00;
pub const WARN: u32 = 0xFAA320;
pub const RED: u32 = 0xFF5A4A;

/// Un thème : la palette complète d'une apparence de KDX (voir THEME_GROUPS dans gen/gen_tokens.py). `light` : texte sombre sur fond clair.
pub struct Theme {
    pub id: &'static str,
    pub nom: &'static str,
    pub light: bool,
    // Surfaces
    pub frame: u32,
    pub panel: u32,
    pub btn: u32,
    pub btn_hover: u32,
    pub card_2: u32,
    pub ink: u32,
    pub col_a: u32,
    pub col_b: u32,
    pub head: u32,
    pub sb_track: u32,
    pub sb_btn: u32,
    pub sb_thumb: u32,
    pub hl: u32,
    // Texte
    pub text: u32,
    pub faint: u32,
    pub btn_text: u32,
    pub head_text: u32,
    pub list_text: u32,
    pub title_text: u32,
    pub tb_text: u32,
    pub hl_text: u32,
    pub field_ink: u32,
    pub own_ink: u32,
    pub sb_arrow: u32,
    // Contours et reliefs
    pub outline: u32,
    pub outline_active: u32,
    pub btn_outline: u32,
    pub field_outline: u32,
    pub line: u32,
    pub hi: u32,
    pub hi2: u32,
    pub lo: u32,
    pub lo2: u32,
    pub field_hi: u32,
    pub field_lo: u32,
    pub row_hover: u32,
    pub row_sel: u32,
    pub row_sel_ring: u32,
    // Cadre inactif
    pub tb_edge: u32,
    pub tb_mid: u32,
    pub tb_low: u32,
    pub tb_from: u32,
    pub tb_grad_mid: u32,
    pub tb_to: u32,
    // Cadre actif et accent
    pub a_edge: u32,
    pub a_mid: u32,
    pub a_dark: u32,
    pub a_hover: u32,
    pub a_title_from: u32,
    pub a_title_mid: u32,
    pub a_title_to: u32,
    pub bar_top: u32,
    pub bar_mid: u32,
    pub bar_bot: u32,
    pub a_spark: u32,
    pub accent: u32,
    pub ic_accent: u32,
    pub ic_accent_dark: u32,
    pub def_outline: u32,
    pub def_edge: u32,
    pub def_mid: u32,
    // Sélection
    pub sel_bg: u32,
    pub sel_fg: u32,
}

/// `THEMES[0]` est le défaut (rouge). Les anciens noms (bleu, vert, violet, charbon, cdx) sont dans `THEME_ALIASES`.
pub const THEMES: [Theme; 16] = [
    Theme { id: "rouge", nom: "Clair-obscur rouge", light: false, frame: 0x222222, panel: 0x333333, btn: 0x333333, btn_hover: 0x3C3C3C, card_2: 0x2A2A2A, ink: 0x000000, col_a: 0x333333, col_b: 0x444444, head: 0x333333, sb_track: 0x2A2A2A, sb_btn: 0x333333, sb_thumb: 0x333333, hl: 0x880000, text: 0xFFFFFF, faint: 0x888888, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0xFFFFFF, title_text: 0xFFFFFF, tb_text: 0xFFFFFF, hl_text: 0xFFFFFF, field_ink: 0x00FF00, own_ink: 0xFFFFFF, sb_arrow: 0x888888, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x555555, hi: 0x666666, hi2: 0x444444, lo: 0x111111, lo2: 0x222222, field_hi: 0x666666, field_lo: 0x111111, row_hover: 0x3C3C3C, row_sel: 0x404040, row_sel_ring: 0x757575, tb_edge: 0x555555, tb_mid: 0x222222, tb_low: 0x111111, tb_from: 0x0C0C0C, tb_grad_mid: 0x222222, tb_to: 0x8F8F8F, a_edge: 0xCC0000, a_mid: 0x880000, a_dark: 0x440000, a_hover: 0x660000, a_title_from: 0x320000, a_title_mid: 0x880000, a_title_to: 0xC30000, bar_top: 0xE83C3C, bar_mid: 0xC01010, bar_bot: 0x8A0202, a_spark: 0xFF8A74, accent: 0xD41818, ic_accent: 0xE02020, ic_accent_dark: 0x8A0202, def_outline: 0x000000, def_edge: 0xCC0000, def_mid: 0x880000, sel_bg: 0x001177, sel_fg: 0xFF00FF },
    Theme { id: "marine", nom: "Marine et rose", light: false, frame: 0x122654, panel: 0x233765, btn: 0x233765, btn_hover: 0x2C406E, card_2: 0x1A2E5C, ink: 0x0F2351, col_a: 0x233765, col_b: 0x374B79, head: 0x122654, sb_track: 0x1A2E5C, sb_btn: 0x233765, sb_thumb: 0x374B79, hl: 0x007FFF, text: 0xCCDCFF, faint: 0x697CA5, btn_text: 0xFF6FA3, head_text: 0xDF598A, list_text: 0xCCDCFF, title_text: 0xD0B1C1, tb_text: 0xCCDCFF, hl_text: 0xFFFFFF, field_ink: 0xCCDCFF, own_ink: 0xFFFFFF, sb_arrow: 0x6E82B0, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x455987, hi: 0x566A98, hi2: 0x344876, lo: 0x011543, lo2: 0x122654, field_hi: 0x566A98, field_lo: 0x011543, row_hover: 0x2C406E, row_sel: 0x304472, row_sel_ring: 0x6275A2, tb_edge: 0x6A7EAC, tb_mid: 0x374B79, tb_low: 0x152957, tb_from: 0x141B2D, tb_grad_mid: 0x374B79, tb_to: 0x9AA4BB, a_edge: 0x9F4E61, a_mid: 0x6C1B2E, a_dark: 0x470009, a_hover: 0x5A0E1C, a_title_from: 0x280A11, a_title_mid: 0x6C1B2E, a_title_to: 0xB58B95, bar_top: 0xB27181, bar_mid: 0x9F4E61, bar_bot: 0x6C1B2E, a_spark: 0xCFA6B0, accent: 0x9F4E61, ic_accent: 0x9F4E61, ic_accent_dark: 0x6C1B2E, def_outline: 0x000000, def_edge: 0x9F4E61, def_mid: 0x6C1B2E, sel_bg: 0x6C1B2E, sel_fg: 0xFFFFFF },
    Theme { id: "ambre", nom: "Ambre CRT", light: false, frame: 0x0A0A0A, panel: 0x0A0A0A, btn: 0x1B1B1B, btn_hover: 0x242424, card_2: 0x121212, ink: 0x448043, col_a: 0x346835, col_b: 0x448044, head: 0x1B1B1B, sb_track: 0x121212, sb_btn: 0x333333, sb_thumb: 0x1B1B1B, hl: 0xB53500, text: 0xB53500, faint: 0x792604, btn_text: 0xE86833, head_text: 0xE86833, list_text: 0x000000, title_text: 0x000000, tb_text: 0xFFFFFF, hl_text: 0x000000, field_ink: 0x000000, own_ink: 0xFFFFFF, sb_arrow: 0x887765, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x3D3D3D, hi: 0x4E4E4E, hi2: 0x2C2C2C, lo: 0x060606, lo2: 0x0A0A0A, field_hi: 0x4E4E4E, field_lo: 0x000000, row_hover: 0x242424, row_sel: 0x282828, row_sel_ring: 0x584C46, tb_edge: 0x555555, tb_mid: 0x222222, tb_low: 0x111111, tb_from: 0x0C0C0C, tb_grad_mid: 0x222222, tb_to: 0x8F8F8F, a_edge: 0xE86833, a_mid: 0xB53500, a_dark: 0x820200, a_hover: 0x9C1C00, a_title_from: 0x431300, a_title_mid: 0xB53500, a_title_to: 0xDA9900, bar_top: 0xED865C, bar_mid: 0xE86833, bar_bot: 0xB53500, a_spark: 0xF4B499, accent: 0xE86833, ic_accent: 0xE86833, ic_accent_dark: 0xB53500, def_outline: 0x000000, def_edge: 0xE86833, def_mid: 0xB53500, sel_bg: 0xB53500, sel_fg: 0xFFFFFF },
    Theme { id: "jean", nom: "Bleu jean", light: false, frame: 0x00002D, panel: 0x00002A, btn: 0x001246, btn_hover: 0x081A4E, card_2: 0x00093A, ink: 0x000015, col_a: 0x001C62, col_b: 0x000015, head: 0x001246, sb_track: 0x00093A, sb_btn: 0x001246, sb_thumb: 0x001246, hl: 0x000016, text: 0xFFFFFF, faint: 0x6A6A83, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0xFFFFFF, title_text: 0xFFFFFF, tb_text: 0xFFFFFF, hl_text: 0xFFFFFF, field_ink: 0xFFFFFF, own_ink: 0xFFFFFF, sb_arrow: 0x4B5D91, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x223468, hi: 0x334579, hi2: 0x112357, lo: 0x00000B, lo2: 0x00002D, field_hi: 0x33335D, field_lo: 0x000000, row_hover: 0x081A4E, row_sel: 0x0D1F53, row_sel_ring: 0x475886, tb_edge: 0x33334F, tb_mid: 0x00001C, tb_low: 0x000000, tb_from: 0x00000A, tb_grad_mid: 0x00001C, tb_to: 0x00008C, a_edge: 0x335FE5, a_mid: 0x002CB2, a_dark: 0x00007F, a_hover: 0x001698, a_title_from: 0x001042, a_title_mid: 0x002CB2, a_title_to: 0x0094D8, bar_top: 0x5C7FEA, bar_mid: 0x335FE5, bar_bot: 0x002CB2, a_spark: 0x99AFF2, accent: 0x335FE5, ic_accent: 0x335FE5, ic_accent_dark: 0x002CB2, def_outline: 0x000000, def_edge: 0x333349, def_mid: 0x000016, sel_bg: 0x002CB2, sel_fg: 0xFFFFFF },
    Theme { id: "tramway", nom: "Tramway", light: false, frame: 0x003684, panel: 0x1A0073, btn: 0x004F9D, btn_hover: 0x0858A6, card_2: 0x004290, ink: 0x000000, col_a: 0x296CBF, col_b: 0x3382D0, head: 0x004F9D, sb_track: 0x004290, sb_btn: 0x004F9D, sb_thumb: 0x004F9D, hl: 0xFF6700, text: 0x00DC97, faint: 0x0F5C82, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0x000000, title_text: 0x000000, tb_text: 0xFFFFFF, hl_text: 0x000000, field_ink: 0x00DC97, own_ink: 0xFFFFFF, sb_arrow: 0x4B9AE8, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x2271BF, hi: 0x3382D0, hi2: 0x1160AE, lo: 0x001C6A, lo2: 0x003684, field_hi: 0x1F52AF, field_lo: 0x00003A, row_hover: 0x0858A6, row_sel: 0x0D5CAA, row_sel_ring: 0x2E8BCA, tb_edge: 0x3C3380, tb_mid: 0x09004D, tb_low: 0x00000E, tb_from: 0x03001C, tb_grad_mid: 0x09004D, tb_to: 0x8200A5, a_edge: 0xFFB34C, a_mid: 0xFF6700, a_dark: 0xCC3400, a_hover: 0xE64E00, a_title_from: 0x5F2600, a_title_mid: 0xFF6700, a_title_to: 0xFFB200, bar_top: 0xFFC270, bar_mid: 0xFFB34C, bar_bot: 0xFF6700, a_spark: 0xFFD9A6, accent: 0xFFB34C, ic_accent: 0xFFB34C, ic_accent_dark: 0xFF6700, def_outline: 0x000000, def_edge: 0xFFB34C, def_mid: 0xFF6700, sel_bg: 0xFF6700, sel_fg: 0x000000 },
    Theme { id: "sarcelle", nom: "Sarcelle", light: false, frame: 0x222244, panel: 0x193333, btn: 0x333355, btn_hover: 0x3C3C5E, card_2: 0x2A2A4C, ink: 0x000000, col_a: 0x333333, col_b: 0x444444, head: 0x333355, sb_track: 0x2A2A4C, sb_btn: 0x333355, sb_thumb: 0x333355, hl: 0x2F7D23, text: 0xFFFFFF, faint: 0x798888, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0xFFFFFF, title_text: 0xFFFFFF, tb_text: 0xFFFFFF, hl_text: 0xFFFFFF, field_ink: 0xFFFFFF, own_ink: 0xFFFFFF, sb_arrow: 0x7E7EA0, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x555577, hi: 0x666688, hi2: 0x444466, lo: 0x111133, lo2: 0x222244, field_hi: 0x4C6666, field_lo: 0x000D0D, row_hover: 0x3C3C5E, row_sel: 0x404062, row_sel_ring: 0x757594, tb_edge: 0x555555, tb_mid: 0x222222, tb_low: 0x111111, tb_from: 0x0C0C0C, tb_grad_mid: 0x222222, tb_to: 0x8F8F8F, a_edge: 0xA233B8, a_mid: 0x6F0085, a_dark: 0x3C0052, a_hover: 0x56006C, a_title_from: 0x290031, a_title_mid: 0x6F0085, a_title_to: 0xB600C1, bar_top: 0xB55CC6, bar_mid: 0xA233B8, bar_bot: 0x6F0085, a_spark: 0xD099DC, accent: 0xA233B8, ic_accent: 0xA233B8, ic_accent_dark: 0x6F0085, def_outline: 0x000000, def_edge: 0x62B056, def_mid: 0x2F7D23, sel_bg: 0x6F0085, sel_fg: 0xFFFFFF },
    Theme { id: "chocolat", nom: "Chocolat framboise", light: false, frame: 0x0A0500, panel: 0x2C1200, btn: 0x0E0600, btn_hover: 0x160E08, card_2: 0x0C0600, ink: 0x0F0600, col_a: 0x563918, col_b: 0x6F5632, head: 0x0E0600, sb_track: 0x0C0600, sb_btn: 0x0E0600, sb_thumb: 0x0E0600, hl: 0x560128, text: 0xB39D7A, faint: 0x644C33, btn_text: 0xE6D0AC, head_text: 0xE6D0AC, list_text: 0xFFFFFF, title_text: 0xFFFFFF, tb_text: 0xFFFFFF, hl_text: 0xFFFFFF, field_ink: 0xFFFFFF, own_ink: 0xFFFFFF, sb_arrow: 0x544C47, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x302822, hi: 0x413933, hi2: 0x1F1711, lo: 0x000000, lo2: 0x0A0500, field_hi: 0x413933, field_lo: 0x000000, row_hover: 0x160E08, row_sel: 0x1B130D, row_sel_ring: 0x4C433A, tb_edge: 0x433348, tb_mid: 0x100015, tb_low: 0x000000, tb_from: 0x050007, tb_grad_mid: 0x100015, tb_to: 0x860088, a_edge: 0x89345B, a_mid: 0x560128, a_dark: 0x240000, a_hover: 0x3D0014, a_title_from: 0x20000E, a_title_mid: 0x560128, a_title_to: 0xA97E92, bar_top: 0xA15D7C, bar_mid: 0x89345B, bar_bot: 0x560128, a_spark: 0xC49AAD, accent: 0x89345B, ic_accent: 0x89345B, ic_accent_dark: 0x560128, def_outline: 0x000000, def_edge: 0x89345B, def_mid: 0x560128, sel_bg: 0x560128, sel_fg: 0xFFFFFF },
    Theme { id: "lavande", nom: "Lavande", light: true, frame: 0xAAAAAA, panel: 0xDDDDDD, btn: 0xDDDDDD, btn_hover: 0xEEEEEE, card_2: 0xC4C4C4, ink: 0xFFFFFF, col_a: 0xDDDDDD, col_b: 0xEEEEEE, head: 0x888888, sb_track: 0xC4C4C4, sb_btn: 0xDDDDDD, sb_thumb: 0x9999FF, hl: 0x555555, text: 0x000000, faint: 0x818181, btn_text: 0x000000, head_text: 0x000000, list_text: 0x000000, title_text: 0x000000, tb_text: 0x777777, hl_text: 0xFFFFFF, field_ink: 0x000000, own_ink: 0x000000, sb_arrow: 0x777777, outline: 0x555555, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0xE8E8E8, hi: 0xEEEEEE, hi2: 0xFFFFFF, lo: 0x777777, lo2: 0xAAAAAA, field_hi: 0xFFFFFF, field_lo: 0x888888, row_hover: 0xEEEEEE, row_sel: 0xF6F6F6, row_sel_ring: 0xD6D6D6, tb_edge: 0xDDDDDD, tb_mid: 0xDDDDDD, tb_low: 0xDDDDDD, tb_from: 0xDDDDDD, tb_grad_mid: 0xDDDDDD, tb_to: 0xDDDDDD, a_edge: 0xFFFFFF, a_mid: 0xCCCCCC, a_dark: 0x999999, a_hover: 0xB2B2B2, a_title_from: 0x666666, a_title_mid: 0xA2A2A2, a_title_to: 0xE9E9E9, bar_top: 0xFFFFFF, bar_mid: 0xFFFFFF, bar_bot: 0xCCCCCC, a_spark: 0xFFFFFF, accent: 0xFFFFFF, ic_accent: 0xFFFFFF, ic_accent_dark: 0xCCCCCC, def_outline: 0x000000, def_edge: 0xDDDDDD, def_mid: 0xAAAAAA, sel_bg: 0xCCCCCC, sel_fg: 0x000000 },
    Theme { id: "oxygene", nom: "Oxygène", light: true, frame: 0x213553, panel: 0x9B999B, btn: 0x324664, btn_hover: 0x3A4E6C, card_2: 0x2A3E5C, ink: 0x666666, col_a: 0x333333, col_b: 0x444444, head: 0x324664, sb_track: 0x2A3E5C, sb_btn: 0x324664, sb_thumb: 0x324664, hl: 0x647896, text: 0xFFFFFF, faint: 0xC5C4C5, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0xFFFFFF, title_text: 0xFFFFFF, tb_text: 0xFFFFFF, hl_text: 0xFFFFFF, field_ink: 0xFFFFFF, own_ink: 0xFFFFFF, sb_arrow: 0x7D91AF, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x546886, hi: 0x657997, hi2: 0x435775, lo: 0x102442, lo2: 0x213553, field_hi: 0xCECCCE, field_lo: 0x797779, row_hover: 0x3A4E6C, row_sel: 0x3F5371, row_sel_ring: 0x7486A1, tb_edge: 0x657997, tb_mid: 0x324664, tb_low: 0x102442, tb_from: 0x121A25, tb_grad_mid: 0x324664, tb_to: 0x97A1B0, a_edge: 0x97ABC9, a_mid: 0x647896, a_dark: 0x425674, a_hover: 0x536785, a_title_from: 0x252C37, a_title_mid: 0x647896, a_title_to: 0xB0BBCA, bar_top: 0xACBCD4, bar_mid: 0x97ABC9, bar_bot: 0x647896, a_spark: 0xCBD5E4, accent: 0x97ABC9, ic_accent: 0x97ABC9, ic_accent_dark: 0x647896, def_outline: 0x000000, def_edge: 0x97ABC9, def_mid: 0x647896, sel_bg: 0x647896, sel_fg: 0xFFFFFF },
    Theme { id: "couchant", nom: "Clair-obscur couchant", light: false, frame: 0x222222, panel: 0x333333, btn: 0x333333, btn_hover: 0x3C3C3C, card_2: 0x2A2A2A, ink: 0x000000, col_a: 0x333333, col_b: 0x444444, head: 0x333333, sb_track: 0x2A2A2A, sb_btn: 0x333333, sb_thumb: 0x333333, hl: 0xCF3F00, text: 0xFFFFFF, faint: 0x888888, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0xFFFFFF, title_text: 0xFFFFFF, tb_text: 0xFFFFFF, hl_text: 0xFFFFFF, field_ink: 0x00FF00, own_ink: 0xFFFFFF, sb_arrow: 0x888888, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x555555, hi: 0x666666, hi2: 0x444444, lo: 0x111111, lo2: 0x222222, field_hi: 0x666666, field_lo: 0x111111, row_hover: 0x3C3C3C, row_sel: 0x404040, row_sel_ring: 0x757575, tb_edge: 0x555555, tb_mid: 0x222222, tb_low: 0x111111, tb_from: 0x0C0C0C, tb_grad_mid: 0x222222, tb_to: 0x8F8F8F, a_edge: 0xFF7334, a_mid: 0xCF3F00, a_dark: 0x9C0C00, a_hover: 0xB62600, a_title_from: 0x4D1700, a_title_mid: 0xCF3F00, a_title_to: 0xE79E00, bar_top: 0xFF8F5D, bar_mid: 0xFF7334, bar_bot: 0xCF3F00, a_spark: 0xFFB99A, accent: 0xFF7334, ic_accent: 0xFF7334, ic_accent_dark: 0xCF3F00, def_outline: 0x000000, def_edge: 0xFF7334, def_mid: 0xCF3F00, sel_bg: 0x001177, sel_fg: 0xFF00FF },
    Theme { id: "techno", nom: "Techno électrique", light: false, frame: 0x333333, panel: 0x424242, btn: 0x444444, btn_hover: 0x4C4C4C, card_2: 0x3C3C3C, ink: 0xD3E7FF, col_a: 0x99CCFF, col_b: 0xD3E7FF, head: 0x444444, sb_track: 0x3C3C3C, sb_btn: 0x424242, sb_thumb: 0x424242, hl: 0x0066FF, text: 0xEFEFEF, faint: 0x8A8A8A, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0x000000, title_text: 0xEDEDED, tb_text: 0x858585, hl_text: 0xFFFFFF, field_ink: 0x000000, own_ink: 0x000000, sb_arrow: 0x333333, outline: 0x4A4A4A, outline_active: 0x0066FF, btn_outline: 0x222222, field_outline: 0x101010, line: 0x666666, hi: 0x777777, hi2: 0x555555, lo: 0x222222, lo2: 0x333333, field_hi: 0x666666, field_lo: 0x222222, row_hover: 0x4C4C4C, row_sel: 0x515151, row_sel_ring: 0x838383, tb_edge: 0x000000, tb_mid: 0x222222, tb_low: 0x000000, tb_from: 0x222222, tb_grad_mid: 0x222222, tb_to: 0x222222, a_edge: 0x555555, a_mid: 0x222222, a_dark: 0x000000, a_hover: 0x111111, a_title_from: 0x9C9C9C, a_title_mid: 0x5A5A5A, a_title_to: 0x101010, bar_top: 0x777777, bar_mid: 0x555555, bar_bot: 0x222222, a_spark: 0xAAAAAA, accent: 0x555555, ic_accent: 0x555555, ic_accent_dark: 0x222222, def_outline: 0x0033CC, def_edge: 0x0066FF, def_mid: 0x5090FF, sel_bg: 0x222222, sel_fg: 0xFFFFFF },
    Theme { id: "reve-bleu", nom: "Rêve bleu", light: true, frame: 0x113C7C, panel: 0x969696, btn: 0x224D8D, btn_hover: 0x2A5696, card_2: 0x1A4484, ink: 0x224D8D, col_a: 0x969696, col_b: 0xB4B4B4, head: 0x224D8D, sb_track: 0x1A4484, sb_btn: 0x004EA3, sb_thumb: 0x224D8D, hl: 0x323232, text: 0x000000, faint: 0x575757, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0x000000, title_text: 0xFFFFFF, tb_text: 0x000000, hl_text: 0xC8C8C8, field_ink: 0xFFFFFF, own_ink: 0xFFFFFF, sb_arrow: 0x4B99EE, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x446FAF, hi: 0x5580C0, hi2: 0x335E9E, lo: 0x002B6B, lo2: 0x113C7C, field_hi: 0xC9C9C9, field_lo: 0x747474, row_hover: 0x2A5696, row_sel: 0x2F5A9A, row_sel_ring: 0x4C73AD, tb_edge: 0xC9C9C9, tb_mid: 0x969696, tb_low: 0x747474, tb_from: 0x373737, tb_grad_mid: 0x969696, tb_to: 0xCACACA, a_edge: 0x5580C0, a_mid: 0x224D8D, a_dark: 0x002B6B, a_hover: 0x113C7C, a_title_from: 0x0C1C34, a_title_mid: 0x224D8D, a_title_to: 0x8FA5C5, bar_top: 0x7799CD, bar_mid: 0x5580C0, bar_bot: 0x224D8D, a_spark: 0xAAC0E0, accent: 0x5580C0, ic_accent: 0x5580C0, ic_accent_dark: 0x224D8D, def_outline: 0x000000, def_edge: 0x656565, def_mid: 0x323232, sel_bg: 0x224D8D, sel_fg: 0xFFFFFF },
    Theme { id: "reve-vert", nom: "Rêve vert", light: true, frame: 0x014E57, panel: 0x969696, btn: 0x125F68, btn_hover: 0x1A6870, card_2: 0x0A5660, ink: 0x125F68, col_a: 0x969696, col_b: 0xB4B4B4, head: 0x125F68, sb_track: 0x0A5660, sb_btn: 0x125F68, sb_thumb: 0x125F68, hl: 0x323232, text: 0x000000, faint: 0x575757, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0x000000, title_text: 0xFFFFFF, tb_text: 0x000000, hl_text: 0xC8C8C8, field_ink: 0xFFFFFF, own_ink: 0xFFFFFF, sb_arrow: 0x5DAAB3, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x34818A, hi: 0x45929B, hi2: 0x237079, lo: 0x00353E, lo2: 0x014E57, field_hi: 0xC9C9C9, field_lo: 0x747474, row_hover: 0x1A6870, row_sel: 0x1F6C75, row_sel_ring: 0x3E838C, tb_edge: 0xC9C9C9, tb_mid: 0x969696, tb_low: 0x747474, tb_from: 0x373737, tb_grad_mid: 0x969696, tb_to: 0xCACACA, a_edge: 0x45929B, a_mid: 0x125F68, a_dark: 0x00353E, a_hover: 0x094A53, a_title_from: 0x062326, a_title_mid: 0x125F68, a_title_to: 0x87AEB3, bar_top: 0x6AA8AF, bar_mid: 0x45929B, bar_bot: 0x125F68, a_spark: 0xA2C8CD, accent: 0x45929B, ic_accent: 0x45929B, ic_accent_dark: 0x125F68, def_outline: 0x000000, def_edge: 0x656565, def_mid: 0x323232, sel_bg: 0x125F68, sel_fg: 0xFFFFFF },
    Theme { id: "reve-violet", nom: "Rêve violet", light: true, frame: 0x3A3A6C, panel: 0x969696, btn: 0x4B4B7D, btn_hover: 0x545486, card_2: 0x424274, ink: 0x323264, col_a: 0x969696, col_b: 0xB4B4B4, head: 0x4B4B7D, sb_track: 0x424274, sb_btn: 0x4B4B7D, sb_thumb: 0x4B4B7D, hl: 0x323232, text: 0x000000, faint: 0x575757, btn_text: 0xFFFFFF, head_text: 0xFFFFFF, list_text: 0x000000, title_text: 0xC8C8C8, tb_text: 0x000000, hl_text: 0xC8C8C8, field_ink: 0xFFFFFF, own_ink: 0xFFFFFF, sb_arrow: 0x2B2B5D, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0x6D6D9F, hi: 0x7E7EB0, hi2: 0x5C5C8E, lo: 0x29295B, lo2: 0x3A3A6C, field_hi: 0xC9C9C9, field_lo: 0x747474, row_hover: 0x545486, row_sel: 0x58588A, row_sel_ring: 0x71719E, tb_edge: 0xC9C9C9, tb_mid: 0x969696, tb_low: 0x747474, tb_from: 0x373737, tb_grad_mid: 0x969696, tb_to: 0xCACACA, a_edge: 0x656597, a_mid: 0x323264, a_dark: 0x101042, a_hover: 0x212153, a_title_from: 0x121225, a_title_mid: 0x323264, a_title_to: 0x9797B0, bar_top: 0x8484AC, bar_mid: 0x656597, bar_bot: 0x323264, a_spark: 0xB2B2CB, accent: 0x656597, ic_accent: 0x656597, ic_accent_dark: 0x323264, def_outline: 0x000000, def_edge: 0x656565, def_mid: 0x323232, sel_bg: 0x323264, sel_fg: 0xFFFFFF },
    Theme { id: "lagon", nom: "Lagon", light: true, frame: 0x8A8A8A, panel: 0xC8C8C8, btn: 0x9B9B9B, btn_hover: 0xA4A4A4, card_2: 0x929292, ink: 0xFFFFFF, col_a: 0xD3FFFF, col_b: 0xFFFFFF, head: 0x9B9B9B, sb_track: 0x929292, sb_btn: 0xC8C8C8, sb_thumb: 0x9B9B9B, hl: 0x0078FF, text: 0x000000, faint: 0x757575, btn_text: 0x000000, head_text: 0x000000, list_text: 0x000000, title_text: 0xFFFFFF, tb_text: 0xFEFFFF, hl_text: 0xFFFFFF, field_ink: 0x000000, own_ink: 0x000000, sb_arrow: 0xFFFFFF, outline: 0xFFFFFF, outline_active: 0xFFFFFF, btn_outline: 0x9B9B9B, field_outline: 0x646464, line: 0xBDBDBD, hi: 0xCECECE, hi2: 0xACACAC, lo: 0x797979, lo2: 0x8A8A8A, field_hi: 0xF0F0F0, field_lo: 0x9B9B9B, row_hover: 0xA4A4A4, row_sel: 0xA8A8A8, row_sel_ring: 0xB9B9B9, tb_edge: 0xFBFBFB, tb_mid: 0xC8C8C8, tb_low: 0xA6A6A6, tb_from: 0x4A4A4A, tb_grad_mid: 0xC8C8C8, tb_to: 0xE3E3E3, a_edge: 0x4CC4FF, a_mid: 0x0078FF, a_dark: 0x0045CC, a_hover: 0x005EE6, a_title_from: 0x002C5F, a_title_mid: 0x0078FF, a_title_to: 0x00BBFF, bar_top: 0x70D0FF, bar_mid: 0x4CC4FF, bar_bot: 0x0078FF, a_spark: 0xA6E2FF, accent: 0x4CC4FF, ic_accent: 0x4CC4FF, ic_accent_dark: 0x0078FF, def_outline: 0xFFFFFF, def_edge: 0x4CC4FF, def_mid: 0x0078FF, sel_bg: 0x0078FF, sel_fg: 0xFFFFFF },
    Theme { id: "turquoise", nom: "Turquoise", light: true, frame: 0xB7B7B7, panel: 0xDDDDDD, btn: 0xCCCCCC, btn_hover: 0xD4D4D4, card_2: 0xC2C2C2, ink: 0xFFFFFF, col_a: 0xF0F0F0, col_b: 0xFFFFFF, head: 0xCCCCCC, sb_track: 0xC2C2C2, sb_btn: 0xCCCCCC, sb_thumb: 0xCCCCCC, hl: 0x3366FF, text: 0x000000, faint: 0x818181, btn_text: 0x000000, head_text: 0x000000, list_text: 0x000000, title_text: 0xFFFFFF, tb_text: 0xFFFFFF, hl_text: 0xFFFFFF, field_ink: 0x000000, own_ink: 0x000000, sb_arrow: 0x888888, outline: 0x000000, outline_active: 0x000000, btn_outline: 0x000000, field_outline: 0x000000, line: 0xEEEEEE, hi: 0xFFFFFF, hi2: 0xDDDDDD, lo: 0x989898, lo2: 0xB7B7B7, field_hi: 0xFFFFFF, field_lo: 0x989898, row_hover: 0xD4D4D4, row_sel: 0xD9D9D9, row_sel_ring: 0xE6E6E6, tb_edge: 0x6A96DE, tb_mid: 0x3763AB, tb_low: 0x154189, tb_from: 0x14243F, tb_grad_mid: 0x3763AB, tb_to: 0x9AB0D5, a_edge: 0x7FB2FF, a_mid: 0x3366FF, a_dark: 0x082DDD, a_hover: 0x1E4AEE, a_title_from: 0x13265F, a_title_mid: 0x3366FF, a_title_to: 0x97B1FF, bar_top: 0x99C1FF, bar_mid: 0x7FB2FF, bar_bot: 0x3366FF, a_spark: 0xBFD8FF, accent: 0x7FB2FF, ic_accent: 0x7FB2FF, ic_accent_dark: 0x3366FF, def_outline: 0x000000, def_edge: 0x7FB2FF, def_mid: 0x3366FF, sel_bg: 0x3366FF, sel_fg: 0xFFFFFF },
];

pub const THEME_ALIASES: [(&str, &str); 5] = [("bleu", "jean"), ("vert", "reve-vert"), ("violet", "reve-violet"), ("charbon", "techno"), ("cdx", "rouge")];

/// Le nom de thème actuel pour un nom donné (alias compris) ; `None` si inconnu.
pub fn theme_id(name: &str) -> Option<&'static str> {
    THEMES.iter().map(|t| t.id).find(|id| *id == name).or_else(|| THEME_ALIASES.iter().find(|(a, _)| *a == name).map(|(_, id)| *id))
}

/// Compatibilité cdx-ui 1.x.
pub type Accent = Theme;
pub const ACCENTS: &[Theme; 16] = &THEMES;

pub const TEXT_SIZE: i32 = 14;
pub const LINE_HEIGHT: i32 = 16;
pub const LOG_LINE_HEIGHT: i32 = 13;
pub const TITLE_HEIGHT: i32 = 21;
pub const FRAME_RING: i32 = 5;
pub const FRAME_OUTLINE: i32 = 1;
pub const TITLE_BUTTON_SIZE: i32 = 14;
pub const SCROLLBAR_SIZE: i32 = 15;
pub const CONTROL_HEIGHT: i32 = 20;
pub const BAR_BUTTON_HEIGHT: i32 = 22;
pub const COLUMN_HEADER_HEIGHT: i32 = 22;
pub const CHECKBOX_SIZE: i32 = 14;
pub const ICON_SIZE: i32 = 16;
pub const SNAP_DISTANCE: i32 = 10;
pub const SNAP_GAP: i32 = 0;
