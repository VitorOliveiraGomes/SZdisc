use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

pub fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Abre a janela nativa do sistema para escolher uma imagem (PNG, JPG, JPEG, BMP).
#[cfg(windows)]
pub fn pick_image_file_dialog() -> Option<String> {
    use std::ffi::OsString;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr::null_mut;
    use winapi::um::commdlg::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    unsafe {
        let mut file_buf = [0u16; 1024];
        let filter_str =
            "Imagens (PNG, JPG, BMP)\0*.png;*.jpg;*.jpeg;*.bmp\0Todos os Arquivos\0*.*\0\0";
        let filter_wide: Vec<u16> = OsString::from(filter_str).encode_wide().collect();
        let title_wide: Vec<u16> = OsString::from("Escolher Imagem\0").encode_wide().collect();

        let mut ofn: OPENFILENAMEW = zeroed();
        ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = null_mut();
        ofn.lpstrFile = file_buf.as_mut_ptr();
        ofn.nMaxFile = file_buf.len() as u32;
        ofn.lpstrFilter = filter_wide.as_ptr();
        ofn.nFilterIndex = 1;
        ofn.lpstrTitle = title_wide.as_ptr();
        ofn.Flags = OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR;

        if GetOpenFileNameW(&mut ofn) != 0 {
            let len = file_buf.iter().position(|&c| c == 0).unwrap_or(0);
            if len > 0 {
                return Some(
                    OsString::from_wide(&file_buf[..len])
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
        None
    }
}

#[cfg(not(windows))]
pub fn pick_image_file_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Imagens", &["png", "jpg", "jpeg", "bmp"])
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}

/// Carrega uma imagem do disco, redimensiona e retorna um Data URL JPEG base64.
pub fn load_image_as_jpeg_data_url(path: &str, target_w: u32, target_h: u32) -> Option<String> {
    if path.trim().is_empty() {
        return None;
    }
    let img = image::open(path).ok()?;
    let resized = img.resize_to_fill(target_w, target_h, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8().into_raw();
    let mut jpeg_buf = Vec::new();
    let mut enc = Encoder::new(&mut jpeg_buf, 85);
    enc.set_sampling_factor(SamplingFactor::F_2_2);
    enc.encode(&rgb, target_w as u16, target_h as u16, ColorType::Rgb)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        encode_base64(&jpeg_buf)
    ))
}
