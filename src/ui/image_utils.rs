use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};
use image::GenericImageView;
#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::mem::{size_of, zeroed};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use winapi::um::commdlg::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

/// Abre a janela nativa do sistema para escolher uma imagem (PNG, JPG, JPEG, BMP)
#[cfg(windows)]
pub fn pick_image_file_dialog() -> Option<String> {
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

/// Carrega uma imagem do disco e aplica máscara circular suave (estilo foto de perfil do Discord)
pub fn load_circular_avatar_texture(ctx: &Context, path: &str) -> Option<TextureHandle> {
    let img = image::open(path).ok()?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }

    let side = w.min(h);
    let crop_x = (w - side) / 2;
    let crop_y = (h - side) / 2;
    let cropped = img.crop_imm(crop_x, crop_y, side, side);
    let resized = cropped.resize_exact(128, 128, image::imageops::FilterType::Triangle);
    let mut rgba = resized.to_rgba8().into_raw();

    let center = 63.5_f32;
    let radius = 63.0_f32;
    for y in 0..128 {
        for x in 0..128 {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = (y * 128 + x) * 4;
            if dist > radius {
                rgba[idx + 3] = 0;
            } else if dist > radius - 1.5 {
                let factor = ((radius - dist) / 1.5).clamp(0.0, 1.0);
                rgba[idx + 3] = (rgba[idx + 3] as f32 * factor) as u8;
            }
        }
    }

    let color_img = ColorImage::from_rgba_unmultiplied([128, 128], &rgba);
    Some(ctx.load_texture("user_custom_avatar", color_img, TextureOptions::LINEAR))
}

/// Carrega uma imagem do disco para usar como Banner do Perfil
pub fn load_banner_texture(ctx: &Context, path: &str) -> Option<TextureHandle> {
    let img = image::open(path).ok()?;
    let resized = img.resize_to_fill(320, 84, image::imageops::FilterType::Triangle);
    let rgba = resized.to_rgba8().into_raw();
    let color_img = ColorImage::from_rgba_unmultiplied([320, 84], &rgba);
    Some(ctx.load_texture("user_custom_banner", color_img, TextureOptions::LINEAR))
}
