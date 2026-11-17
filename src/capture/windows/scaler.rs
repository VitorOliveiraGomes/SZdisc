use super::super::RawFrame;

pub fn compute_output_dimensions(
    src_w: i32,
    src_h: i32,
    target_w: u32,
    target_h: u32,
) -> Option<(u32, u32, f32)> {
    if src_w < 2 || src_h < 2 {
        return None;
    }
    let scale_x = target_w as f32 / src_w as f32;
    let scale_y = target_h as f32 / src_h as f32;
    let scale = scale_x.min(scale_y).min(1.0);

    let out_w = (((src_w as f32 * scale) as u32) & !1).max(2);
    let out_h = (((src_h as f32 * scale) as u32) & !1).max(2);
    Some((out_w, out_h, scale))
}

/// Converte BGRA capturado em `RawFrame` (1:1 rápido ou downscale bilinear 2x2 inteiro).
/// Quando `capture_rgb == false` (ex.: miniaturas), evita alocar e preencher o buffer RGB.
pub fn convert_bgra_to_frame(
    bgra: &[u8],
    source_size: (i32, i32),
    output_size: (u32, u32),
    capture_rgb: bool,
    capture_rgba: bool,
    spare_rgb: &mut Vec<u8>,
) -> Option<RawFrame> {
    let (src_w, src_h) = source_size;
    let (out_w, out_h) = output_size;
    if src_w < 2 || src_h < 2 || out_w < 2 || out_h < 2 {
        return None;
    }
    let expected_bgra = (src_w as usize) * (src_h as usize) * 4;
    if bgra.len() < expected_bgra {
        return None;
    }

    let out_pixels = (out_w * out_h) as usize;
    let mut rgb = if capture_rgb {
        let mut buf = std::mem::take(spare_rgb);
        if buf.len() != out_pixels * 3 {
            buf.resize(out_pixels * 3, 0);
        }
        buf
    } else {
        Vec::new()
    };

    let mut rgba = if capture_rgba {
        vec![0u8; out_pixels * 4]
    } else {
        Vec::new()
    };

    if out_w as i32 == src_w && out_h as i32 == src_h {
        let bgra = &bgra[..expected_bgra];
        match (capture_rgb, capture_rgba) {
            (true, true) => {
                for ((bpx, rgb_px), rgba_px) in bgra
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(rgb.as_chunks_mut::<3>().0.iter_mut())
                    .zip(rgba.as_chunks_mut::<4>().0.iter_mut())
                {
                    let (b, g, r) = (bpx[0], bpx[1], bpx[2]);
                    rgb_px[0] = r;
                    rgb_px[1] = g;
                    rgb_px[2] = b;
                    rgba_px[0] = r;
                    rgba_px[1] = g;
                    rgba_px[2] = b;
                    rgba_px[3] = 255;
                }
            }
            (true, false) => {
                for (bpx, rgb_px) in bgra
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(rgb.as_chunks_mut::<3>().0.iter_mut())
                {
                    rgb_px[0] = bpx[2];
                    rgb_px[1] = bpx[1];
                    rgb_px[2] = bpx[0];
                }
            }
            (false, true) => {
                for (bpx, rgba_px) in bgra
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(rgba.as_chunks_mut::<4>().0.iter_mut())
                {
                    rgba_px[0] = bpx[2];
                    rgba_px[1] = bpx[1];
                    rgba_px[2] = bpx[0];
                    rgba_px[3] = 255;
                }
            }
            (false, false) => {}
        }
    } else {
        let src_row_stride = (src_w as usize) * 4;
        let out_w_usize = out_w as usize;
        let out_h_usize = out_h as usize;
        let max_sx = (src_w as usize).saturating_sub(2);
        let max_sy = (src_h as usize).saturating_sub(2);
        let scale_x_fp = ((src_w as usize) << 16) / out_w_usize.max(1);
        let scale_y_fp = ((src_h as usize) << 16) / out_h_usize.max(1);

        for dy in 0..out_h_usize {
            let sy_fp = dy * scale_y_fp;
            let sy = (sy_fp >> 16).min(max_sy);
            let wy = (sy_fp & 0xFFFF) >> 8;
            let inv_wy = 256 - wy;

            let row0 = &bgra[sy * src_row_stride..(sy + 1) * src_row_stride];
            let row1 = &bgra[(sy + 1) * src_row_stride..(sy + 2) * src_row_stride];

            for dx in 0..out_w_usize {
                let sx_fp = dx * scale_x_fp;
                let sx = (sx_fp >> 16).min(max_sx);
                let wx = (sx_fp & 0xFFFF) >> 8;
                let inv_wx = 256 - wx;

                let i0 = sx * 4;
                let i1 = i0 + 4;
                let w00 = inv_wx * inv_wy;
                let w10 = wx * inv_wy;
                let w01 = inv_wx * wy;
                let w11 = wx * wy;

                let b = ((row0[i0] as usize * w00
                    + row0[i1] as usize * w10
                    + row1[i0] as usize * w01
                    + row1[i1] as usize * w11)
                    >> 16) as u8;
                let g = ((row0[i0 + 1] as usize * w00
                    + row0[i1 + 1] as usize * w10
                    + row1[i0 + 1] as usize * w01
                    + row1[i1 + 1] as usize * w11)
                    >> 16) as u8;
                let r = ((row0[i0 + 2] as usize * w00
                    + row0[i1 + 2] as usize * w10
                    + row1[i0 + 2] as usize * w01
                    + row1[i1 + 2] as usize * w11)
                    >> 16) as u8;

                let px_idx = dy * out_w_usize + dx;
                if capture_rgb {
                    let r_off = px_idx * 3;
                    rgb[r_off] = r;
                    rgb[r_off + 1] = g;
                    rgb[r_off + 2] = b;
                }
                if capture_rgba {
                    let a_off = px_idx * 4;
                    rgba[a_off] = r;
                    rgba[a_off + 1] = g;
                    rgba[a_off + 2] = b;
                    rgba[a_off + 3] = 255;
                }
            }
        }
    }

    Some(RawFrame {
        width: out_w,
        height: out_h,
        rgb,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miniatura_nao_aloca_rgb_e_respeita_dimensoes_pares() {
        let (w, h, _) = compute_output_dimensions(1920, 1080, 272, 153).unwrap();
        assert_eq!(w % 2, 0);
        assert_eq!(h % 2, 0);
        assert!(w <= 272 && h <= 153);

        let bgra = vec![128u8; 4 * 4 * 4];
        let mut spare = Vec::new();
        let frame = convert_bgra_to_frame(&bgra, (4, 4), (2, 2), false, true, &mut spare).unwrap();
        assert!(frame.rgb.is_empty());
        assert_eq!(frame.rgba.len(), 2 * 2 * 4);
    }
}
