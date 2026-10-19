//! REXPaint `.xp` images: gzip around a little-endian grid of cells, stored
//! column by column. The original loads only the first layer of each
//! (`LoadXP` and `xp_loader.load_layer_to_console`), transparent cells
//! included, so this does the same. Taken over from Armoured Commander I,
//! with glyph codes up to 287.

use crate::color::Color;
use crate::console::{Cell, Console};
use flate2::read::GzDecoder;
use std::io::Read;

/// Every `.xp` file in the original `data/`, built into the binary.
pub const FILES: &[(&str, &[u8])] = &[
    ("attack_bkg.xp", include_bytes!("../data/attack_bkg.xp")),
    ("bkg.xp", include_bytes!("../data/bkg.xp")),
    ("calendar_bkg.xp", include_bytes!("../data/calendar_bkg.xp")),
    ("cats.xp", include_bytes!("../data/cats.xp")),
    ("dayhex_openground.xp", include_bytes!("../data/dayhex_openground.xp")),
    ("dayhex_openground_snow.xp", include_bytes!("../data/dayhex_openground_snow.xp")),
    ("daymap_bkg.xp", include_bytes!("../data/daymap_bkg.xp")),
    ("flag_CAN.xp", include_bytes!("../data/flag_CAN.xp")),
    ("flag_FIN.xp", include_bytes!("../data/flag_FIN.xp")),
    ("flag_FRA.xp", include_bytes!("../data/flag_FRA.xp")),
    ("flag_GER.xp", include_bytes!("../data/flag_GER.xp")),
    ("flag_POL.xp", include_bytes!("../data/flag_POL.xp")),
    ("flag_SOV.xp", include_bytes!("../data/flag_SOV.xp")),
    ("flag_UK.xp", include_bytes!("../data/flag_UK.xp")),
    ("flag_USA.xp", include_bytes!("../data/flag_USA.xp")),
    ("hex_fov.xp", include_bytes!("../data/hex_fov.xp")),
    ("hex_highlight.xp", include_bytes!("../data/hex_highlight.xp")),
    ("hex_objective.xp", include_bytes!("../data/hex_objective.xp")),
    ("main_title.xp", include_bytes!("../data/main_title.xp")),
    ("poppy.xp", include_bytes!("../data/poppy.xp")),
    ("popup_bkg.xp", include_bytes!("../data/popup_bkg.xp")),
    ("scen_hex.xp", include_bytes!("../data/scen_hex.xp")),
    ("scen_hex_fov.xp", include_bytes!("../data/scen_hex_fov.xp")),
    ("scen_hex_snow.xp", include_bytes!("../data/scen_hex_snow.xp")),
    ("tile_offmap.xp", include_bytes!("../data/tile_offmap.xp")),
    ("unit_105mm_gun.xp", include_bytes!("../data/unit_105mm_gun.xp")),
    ("unit_18_pdr.xp", include_bytes!("../data/unit_18_pdr.xp")),
    ("unit_25_pdr.xp", include_bytes!("../data/unit_25_pdr.xp")),
    ("unit_25mm_gun.xp", include_bytes!("../data/unit_25mm_gun.xp")),
    ("unit_2_pdr.xp", include_bytes!("../data/unit_2_pdr.xp")),
    ("unit_37mm_gun.xp", include_bytes!("../data/unit_37mm_gun.xp")),
    ("unit_45mm_gun.xp", include_bytes!("../data/unit_45mm_gun.xp")),
    ("unit_4_5_inch_howitzer.xp", include_bytes!("../data/unit_4_5_inch_howitzer.xp")),
    ("unit_508_taczanka.xp", include_bytes!("../data/unit_508_taczanka.xp")),
    ("unit_621_truck.xp", include_bytes!("../data/unit_621_truck.xp")),
    ("unit_75mm_aa_gun.xp", include_bytes!("../data/unit_75mm_aa_gun.xp")),
    ("unit_75mm_gun.xp", include_bytes!("../data/unit_75mm_gun.xp")),
    ("unit_76mm_gun.xp", include_bytes!("../data/unit_76mm_gun.xp")),
    ("unit_7TP.xp", include_bytes!("../data/unit_7TP.xp")),
    ("unit_B-1_bis.xp", include_bytes!("../data/unit_B-1_bis.xp")),
    ("unit_BA-20.xp", include_bytes!("../data/unit_BA-20.xp")),
    ("unit_BA-6.xp", include_bytes!("../data/unit_BA-6.xp")),
    ("unit_FT-17C.xp", include_bytes!("../data/unit_FT-17C.xp")),
    ("unit_FT-17M.xp", include_bytes!("../data/unit_FT-17M.xp")),
    ("unit_H35.xp", include_bytes!("../data/unit_H35.xp")),
    ("unit_H35_late.xp", include_bytes!("../data/unit_H35_late.xp")),
    ("unit_R35_late.xp", include_bytes!("../data/unit_R35_late.xp")),
    ("unit_S20TL.xp", include_bytes!("../data/unit_S20TL.xp")),
    ("unit_S35.xp", include_bytes!("../data/unit_S35.xp")),
    ("unit_SPW_251_1.xp", include_bytes!("../data/unit_SPW_251_1.xp")),
    ("unit_SPW_251_10.xp", include_bytes!("../data/unit_SPW_251_10.xp")),
    ("unit_SU-12.xp", include_bytes!("../data/unit_SU-12.xp")),
    ("unit_TK3.xp", include_bytes!("../data/unit_TK3.xp")),
    ("unit_TKS.xp", include_bytes!("../data/unit_TKS.xp")),
    ("unit_TKS_20mm.xp", include_bytes!("../data/unit_TKS_20mm.xp")),
    ("unit_ZIS-5.xp", include_bytes!("../data/unit_ZIS-5.xp")),
    ("unit_a10_mk2.xp", include_bytes!("../data/unit_a10_mk2.xp")),
    ("unit_a13_mk1.xp", include_bytes!("../data/unit_a13_mk1.xp")),
    ("unit_a13_mk2.xp", include_bytes!("../data/unit_a13_mk2.xp")),
    ("unit_a9_mk1.xp", include_bytes!("../data/unit_a9_mk1.xp")),
    ("unit_a9_mk1_cs.xp", include_bytes!("../data/unit_a9_mk1_cs.xp")),
    ("unit_amd_35.xp", include_bytes!("../data/unit_amd_35.xp")),
    ("unit_atr_team.xp", include_bytes!("../data/unit_atr_team.xp")),
    ("unit_bt5_m34.xp", include_bytes!("../data/unit_bt5_m34.xp")),
    ("unit_bt7_m37.xp", include_bytes!("../data/unit_bt7_m37.xp")),
    ("unit_bt7a.xp", include_bytes!("../data/unit_bt7a.xp")),
    ("unit_buessing_nag_4500_truck.xp", include_bytes!("../data/unit_buessing_nag_4500_truck.xp")),
    ("unit_citroen_23.xp", include_bytes!("../data/unit_citroen_23.xp")),
    ("unit_daimler_sc.xp", include_bytes!("../data/unit_daimler_sc.xp")),
    ("unit_firefly.xp", include_bytes!("../data/unit_firefly.xp")),
    ("unit_flak_36.xp", include_bytes!("../data/unit_flak_36.xp")),
    ("unit_ft_team.xp", include_bytes!("../data/unit_ft_team.xp")),
    ("unit_hmg.xp", include_bytes!("../data/unit_hmg.xp")),
    ("unit_jgdpz_38t.xp", include_bytes!("../data/unit_jgdpz_38t.xp")),
    ("unit_jgdpz_iv.xp", include_bytes!("../data/unit_jgdpz_iv.xp")),
    ("unit_light_arty_car.xp", include_bytes!("../data/unit_light_arty_car.xp")),
    ("unit_locomotive.xp", include_bytes!("../data/unit_locomotive.xp")),
    ("unit_m4.xp", include_bytes!("../data/unit_m4.xp")),
    ("unit_m4a1.xp", include_bytes!("../data/unit_m4a1.xp")),
    ("unit_m4a1_76.xp", include_bytes!("../data/unit_m4a1_76.xp")),
    ("unit_m4a3_76.xp", include_bytes!("../data/unit_m4a3_76.xp")),
    ("unit_marder_II.xp", include_bytes!("../data/unit_marder_II.xp")),
    ("unit_marder_III_m.xp", include_bytes!("../data/unit_marder_III_m.xp")),
    ("unit_matilda.xp", include_bytes!("../data/unit_matilda.xp")),
    ("unit_matilda_ii.xp", include_bytes!("../data/unit_matilda_ii.xp")),
    ("unit_mk_6.xp", include_bytes!("../data/unit_mk_6.xp")),
    ("unit_mk_6b.xp", include_bytes!("../data/unit_mk_6b.xp")),
    ("unit_morris_15_cwt_truck.xp", include_bytes!("../data/unit_morris_15_cwt_truck.xp")),
    ("unit_morris_cs9.xp", include_bytes!("../data/unit_morris_cs9.xp")),
    ("unit_motorcycle_mg.xp", include_bytes!("../data/unit_motorcycle_mg.xp")),
    ("unit_opel_3t_truck.xp", include_bytes!("../data/unit_opel_3t_truck.xp")),
    ("unit_oqf_37_aa.xp", include_bytes!("../data/unit_oqf_37_aa.xp")),
    ("unit_pak_35-36.xp", include_bytes!("../data/unit_pak_35-36.xp")),
    ("unit_pak_38.xp", include_bytes!("../data/unit_pak_38.xp")),
    ("unit_pak_40.xp", include_bytes!("../data/unit_pak_40.xp")),
    ("unit_pak_43.xp", include_bytes!("../data/unit_pak_43.xp")),
    ("unit_psw_221.xp", include_bytes!("../data/unit_psw_221.xp")),
    ("unit_psw_222.xp", include_bytes!("../data/unit_psw_222.xp")),
    ("unit_psw_231_6rad.xp", include_bytes!("../data/unit_psw_231_6rad.xp")),
    ("unit_psw_231_8rad.xp", include_bytes!("../data/unit_psw_231_8rad.xp")),
    ("unit_psw_234_2.xp", include_bytes!("../data/unit_psw_234_2.xp")),
    ("unit_pz_35t.xp", include_bytes!("../data/unit_pz_35t.xp")),
    ("unit_pz_38t_a.xp", include_bytes!("../data/unit_pz_38t_a.xp")),
    ("unit_pz_II.xp", include_bytes!("../data/unit_pz_II.xp")),
    ("unit_pz_III_D.xp", include_bytes!("../data/unit_pz_III_D.xp")),
    ("unit_pz_III_F.xp", include_bytes!("../data/unit_pz_III_F.xp")),
    ("unit_pz_IV_A.xp", include_bytes!("../data/unit_pz_IV_A.xp")),
    ("unit_pz_IV_C.xp", include_bytes!("../data/unit_pz_IV_C.xp")),
    ("unit_pz_IV_D.xp", include_bytes!("../data/unit_pz_IV_D.xp")),
    ("unit_pz_IV_H.xp", include_bytes!("../data/unit_pz_IV_H.xp")),
    ("unit_pz_I_B.xp", include_bytes!("../data/unit_pz_I_B.xp")),
    ("unit_pz_VG.xp", include_bytes!("../data/unit_pz_VG.xp")),
    ("unit_pz_VI_b.xp", include_bytes!("../data/unit_pz_VI_b.xp")),
    ("unit_pz_VI_e.xp", include_bytes!("../data/unit_pz_VI_e.xp")),
    ("unit_pz_jg_i.xp", include_bytes!("../data/unit_pz_jg_i.xp")),
    ("unit_riflemen.xp", include_bytes!("../data/unit_riflemen.xp")),
    ("unit_sd_kfz_13.xp", include_bytes!("../data/unit_sd_kfz_13.xp")),
    ("unit_stug_iiib.xp", include_bytes!("../data/unit_stug_iiib.xp")),
    ("unit_stug_iiig.xp", include_bytes!("../data/unit_stug_iiig.xp")),
    ("unit_t26_m39.xp", include_bytes!("../data/unit_t26_m39.xp")),
    ("unit_t28_m34.xp", include_bytes!("../data/unit_t28_m34.xp")),
    ("unit_unknown.xp", include_bytes!("../data/unit_unknown.xp")),
    ("unit_vickers_ejw.xp", include_bytes!("../data/unit_vickers_ejw.xp")),
    ("unit_wz_34_37.xp", include_bytes!("../data/unit_wz_34_37.xp")),
    ("unit_wz_34_MG.xp", include_bytes!("../data/unit_wz_34_MG.xp")),
];

#[allow(dead_code)]
pub fn by_name(name: &str) -> Option<Console> {
    FILES.iter().find(|(n, _)| *n == name).and_then(|(_, data)| load(data).ok())
}

pub fn load(gz: &[u8]) -> Result<Console, String> {
    let mut raw = Vec::new();
    GzDecoder::new(gz).read_to_end(&mut raw).map_err(|e| format!("gzip: {e}"))?;
    let mut at = 0;
    let mut int = |raw: &[u8]| -> Result<i32, String> {
        let b = raw.get(at..at + 4).ok_or("truncated")?;
        at += 4;
        Ok(i32::from_le_bytes(b.try_into().unwrap()))
    };
    let _version = int(&raw)?;
    let layers = int(&raw)?;
    if layers < 1 {
        return Err("no layers".into());
    }
    let w = int(&raw)?;
    let h = int(&raw)?;
    let start = 16;
    let need = start + (w * h) as usize * 10;
    if raw.len() < need {
        return Err("truncated layer".into());
    }
    let mut con = Console::new(w, h);
    for i in 0..(w * h) as usize {
        let c = &raw[start + i * 10..start + i * 10 + 10];
        let code = u32::from_le_bytes(c[0..4].try_into().unwrap());
        let cell = Cell {
            ch: code.min(crate::glyph::GLYPHS as u32 - 1) as u16,
            fg: Color(c[4], c[5], c[6]),
            bg: Color(c[7], c[8], c[9]),
        };
        // Column-major: cell i is at x = i / h, y = i % h.
        con.set_cell(i as i32 / h, i as i32 % h, cell);
    }
    Ok(con)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_loads() {
        for (name, data) in FILES {
            let con = load(data).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(con.w > 0 && con.h > 0, "{name}");
        }
    }

    #[test]
    fn sizes_match_the_originals() {
        assert_eq!(by_name("main_title.xp").map(|c| (c.w, c.h)), Some((90, 60)));
        assert_eq!(by_name("unit_m4.xp").map(|c| (c.w, c.h)), Some((24, 8)));
        assert_eq!(FILES.len(), 125);
    }
}
