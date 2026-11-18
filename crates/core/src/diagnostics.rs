//! Smart error diagnostics: turns opaque C/C++ engine stderr into structured, bilingual
//! (English + Arabic) actionable guidance.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnosis {
    pub code: &'static str,
    pub message_en: &'static str,
    pub message_ar: &'static str,
    pub hint_en: &'static str,
    pub hint_ar: &'static str,
}

/// Inspects `tool` and raw `stderr` output for known failure causes.
pub fn diagnose(tool: &str, stderr: &str) -> Option<Diagnosis> {
    let lower = stderr.to_ascii_lowercase();

    if lower.contains("drm")
        || lower.contains("adept")
        || lower.contains("incorrect password")
        || lower.contains("wrong password")
        || lower.contains("invalid password")
        || lower.contains("password required")
        || lower.contains("file is encrypted")
        || lower.contains("data error in encrypted file")
    {
        return Some(Diagnosis {
            code: "drm_or_password",
            message_en: "File is encrypted or DRM-protected.",
            message_ar: "الملف مشفّر أو محمي بإدارة الحقوق الرقمية (DRM).",
            hint_en: "Provide the password (Unlock PDF / Extract) or use a DRM-free copy.",
            hint_ar: "أدخل كلمة المرور (فتح PDF / فك الأرشيف) أو استخدم نسخة غير محمية بـ DRM.",
        });
    }

    if lower.contains("error opening data file")
        || lower.contains("tessdata_prefix")
        || lower.contains("failed loading language")
    {
        return Some(Diagnosis {
            code: "missing_tessdata",
            message_en: "Tesseract language data (.traineddata) could not be loaded.",
            message_ar: "تعذّر تحميل بيانات لغة Tesseract ‏(.traineddata).",
            hint_en: "Check that eng.traineddata / ara.traineddata exist in tessdata and TESSDATA_PREFIX has no \\\\?\\ prefix.",
            hint_ar: "تأكّد من وجود ملفات eng.traineddata و ara.traineddata في مجلد tessdata وأن مسار TESSDATA_PREFIX سليم.",
        });
    }

    if lower.contains("cannot find font")
        || lower.contains("fontconfig error")
        || lower.contains("no fonts found")
        || lower.contains("substituting font")
        || (tool.to_ascii_lowercase().contains("libreoffice") && lower.contains("font"))
    {
        return Some(Diagnosis {
            code: "missing_fonts",
            message_en: "Required document fonts are missing on this system.",
            message_ar: "بعض الخطوط المطلوبة للمستند غير مثبّتة على هذا النظام.",
            hint_en: "Install the document's fonts (or Arabic/Liberation fonts) or embed fonts before converting.",
            hint_ar: "ثبّت خطوط المستند المطلوبة (مثل الخطوط العربية أو Liberation) أو ضمّن الخطوط قبل التحويل.",
        });
    }

    if lower.contains("invalid data found when processing input")
        || lower.contains("moov atom not found")
        || lower.contains("corrupt")
        || lower.contains("damaged")
        || lower.contains("unexpected eof")
        || lower.contains("premature end of")
        || lower.contains("not a jpeg file")
        || lower.contains("not a png file")
        || lower.contains("unable to find an xref")
    {
        return Some(Diagnosis {
            code: "corrupted_input",
            message_en: "The input file header appears corrupted or incomplete.",
            message_ar: "يبدو أن ترويسة الملف المدخل تالفة أو غير مكتملة.",
            hint_en: "Verify the source file opens normally and is completely downloaded before converting.",
            hint_ar: "تحقّق من سلامة الملف الأصلي واكتمال تنزيله قبل إعادة التحويل.",
        });
    }

    if lower.contains("no nvenc capable devices")
        || lower.contains("cannot load nvcuda")
        || lower.contains("vainitialize")
        || lower.contains("device creation failed")
        || lower.contains("hwaccel")
    {
        return Some(Diagnosis {
            code: "hw_encoder_unavailable",
            message_en: "Hardware GPU encoder is unavailable for this stream.",
            message_ar: "مسرّع البطاقة الرسومية (GPU) غير متاح لهذا الملف.",
            hint_en: "Zeidex will fall back to CPU encoding, or switch Hardware Acceleration to Off in Settings / --hwaccel off.",
            hint_ar: "سيتم التحويل عبر المعالج (CPU)، أو أوقف تسريع العتاد من الإعدادات (--hwaccel off).",
        });
    }

    if lower.contains("no space left on device") || lower.contains("not enough space on the disk") {
        return Some(Diagnosis {
            code: "disk_full",
            message_en: "Not enough free disk space to write the output.",
            message_ar: "لا توجد مساحة تخزين كافية على القرص لحفظ الناتج.",
            hint_en: "Free up space on the destination drive and try again.",
            hint_ar: "حرّر مساحة كافية على القرص الهدف ثم أعد المحاولة.",
        });
    }

    None
}

/// Formats a tool failure with an actionable bilingual diagnosis when matched, falling back to
/// a bounded tail of stderr.
pub fn format_failure(tool: &str, status_note: &str, stderr: &str) -> String {
    let trimmed = stderr.trim();
    if let Some(d) = diagnose(tool, trimmed) {
        return format!("[{}] {} {} | {} {}", d.code, d.message_en, d.hint_en, d.message_ar, d.hint_ar);
    }
    let tail: String = trimmed.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
    if status_note.is_empty() {
        tail
    } else if tail.is_empty() {
        status_note.to_string()
    } else {
        format!("{status_note}: {tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_common_engine_failures() {
        let drm = diagnose("Calibre", "ERROR: DRM protected book cannot be converted").unwrap();
        assert_eq!(drm.code, "drm_or_password");

        let corrupt = diagnose("FFmpeg", "moov atom not found\nInvalid data found when processing input").unwrap();
        assert_eq!(corrupt.code, "corrupted_input");

        let tess = diagnose("Tesseract", "Error opening data file tessdata/ara.traineddata").unwrap();
        assert_eq!(tess.code, "missing_tessdata");

        let fonts = diagnose("LibreOffice", "Fontconfig error: Cannot find font 'Amiri'").unwrap();
        assert_eq!(fonts.code, "missing_fonts");

        let formatted = format_failure("Tesseract", "exit Some(1)", "Failed loading language 'ara'");
        assert!(formatted.contains("[missing_tessdata]"));
        assert!(formatted.contains("Tesseract"));
    }
}
