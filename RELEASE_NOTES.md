## ما الجديد في 0.3.4

- ويندوز لم يعد يقول «LibreOffice not found»: LibreOffice 26.2.7 وCalibre 9.18.0 يُضمَّنان في حزمة التثبيت، وتُفكّان بـ`msiexec /a` لأن كابينة الـMSI تحفظ الملفات مسطّحة بلا شجرة `program/` و`share/`. ثم يُقلَّم ما لا يستخدمه التحويل بلا واجهة: صفحات المساعدة وسِمات الأيقونات و56 قاموس تدقيق إملائي (يبقى الإنجليزي والعربي)، وتُنقل خطوط الـMSI إلى `share/fonts/truetype` ليبقى Caladea/Carlito على مقاييس Calibri/Cambria وAmiri للتشكيل العربي. البحث عن المحرّك صار يشمل `LOCALAPPDATA\Programs` و`ProgramW6432` و`ProgramFiles(x86)`، وعلى macOS حزمة `.app`، وعلى لينكس `/usr/lib64` و`/opt/libreoffice*`.
- العجلة المنبثقة تعرض الأدوات فعلاً: حدث `wheel-open` كان يفعّل وضع النقر فيمنع أحداث السحب الأصلية، فأُضيف `wheel-files` للسحب، وتُسلَّم مسارات الملفات من الكرة، ويُخفى الوجه إذا كان فارغًا، وتُغلق العجلة بعد 3.2 ثانية خمول مع نبضة `wheel-alive` أثناء السحب البطيء.
- الأدوات التي لا محرّك لها لا تُعرض ثم تفشل: `describe` و`convert` يرشّحان `ocr` و`exif` و`qr-read` و`make-qr` و`transcribe` و`upscale` و`remove-bg` و`merge-pdf`… حسب وجود المحرّك والطراز.
- حزمة ويندوز تحمل أيضًا Tesseract (eng+ara+osd) وqrencode وExifTool وZBar وwhisper.cpp وReal-ESRGAN وطرازَي ggml-tiny وu2net. حزم لينكس وmacOS تحمل ما عدا LibreOffice وCalibre وwhisper على macOS.
- زر «تثبيت» في لوحة Engines يشغّل أمر winget/brew/apt في طرفية مرئية بدل ترك المستخدم أمام رسالة خطأ.
- أداة «إنشاء رمز QR» (qrencode) تعمل من العجلة، وقراءة QR تظهر فقط عند وجود ZBar.

## What changed in 0.3.4

- Windows no longer says `LibreOffice not found`: LibreOffice 26.2.7 and Calibre 9.18.0 travel inside the setup, unpacked with `msiexec /a` because the MSI cabinet stores files flat and has no `program/` + `share/` tree to copy. The staged tree is trimmed of help pages, icon themes and 56 spelling dictionaries (English and Arabic stay), and the installer's `Fonts/` folder is re-homed to `share/fonts/truetype`, so Caladea/Carlito keep Calibri/Cambria metrics and Amiri keeps Arabic shaping. Engine lookup now covers `LOCALAPPDATA\Programs`, `ProgramW6432` and `ProgramFiles(x86)` on Windows, `.app` bundles on macOS, and `/usr/lib64` plus `/opt/libreoffice*` on Linux.
- The drag wheel really shows the tools: `wheel-open` used to enable click mode and swallow native drop events, so `wheel-files` now carries the paths, the face is hidden when it would be empty, and the wheel closes after 3.2 s idle with `wheel-alive` heartbeats during slow drags.
- Tools with no engine are not offered and then failed: `describe` and `convert` filter `ocr`, `exif`, `qr-read`, `make-qr`, `transcribe`, `upscale`, `remove-bg`, `merge-pdf` and the rest by engine and model presence.
- The Windows setup also carries Tesseract (eng+ara+osd), qrencode, ExifTool, ZBar, whisper.cpp, Real-ESRGAN and the ggml-tiny and u2net models. Linux and macOS carry everything except LibreOffice and Calibre, and whisper on macOS.
- Every missing engine has an Install button in the Engines panel: it opens a visible terminal and runs the winget/brew/apt command instead of leaving the user with an error message.
- The Make QR code tool (qrencode) works from the wheel; QR reading appears only when ZBar is present.

## ما الجديد في 0.3.3

- الحزم تحمل أدوات OCR وQR والتحرير التلقائي: libvips وTesseract وqrencode وlibgmp داخل الحزمة مع بيانات eng وara وosd، فلم يعد جهاز نظيف يحتاج تثبيتًا خارجيًا (‎+164 MiB).
- على ويندوز يُنزَّل FFmpeg وlibvips برابط وإصدار وبصمة SHA-256 ثابتة (FFmpeg n9.0.2، libvips 8.18.7) بدل «أحدث إصدار» بلا تحقّق، ولم يعد رمز GitHub يُرسل إلى conda.anaconda.org.
- أمان: صلاحية الكتابة أُزيلت من كل مهام CI إلا التي تنشر، وtokio 1.54.1 وrustix 1.1.6 يغلقان RUSTSEC-2026-0384 وRUSTSEC-2026-0486.
- شاشة «حول» تعرض الإصدار الحقيقي من package.json بدل 0.1.0، ومفتاح macOS يشرح صلاحية Accessibility بدل لوم Wayland، وEscape يغلق نافذتي «حول» و«تبرّع»، ونافذة التبرّع عربية كاملة.
- فحص الحزمة وأدوات النظام مخفية: 81 ناجحًا، 0 فشل، 3 متخطاة (ExifTool وقراءة QR اختياريان).
- أندرويد 1.3.0 (في مستودع zeidex-convert-android): تحويل أي مستند إلى PDF على الجهاز — DOCX وXLSX/ODS/CSV وPPTX وODT وRTF وHTML وEPUB وMarkdown إلى PDF بنص قابل للتحديد وجداول مرسومة، مع PDF إلى DOCX.

الملفات: Linux AppImage (x64 وarm64)، Windows x64 setup، macOS DMG (Apple Silicon وIntel)، ولكل ملف بصمته في `SHA256SUMS-*.txt`.

## What changed in 0.3.3

- Packages now carry the OCR, QR and auto-redaction engines: libvips, Tesseract, qrencode and libgmp travel inside with the eng, ara and osd data, so a clean machine needs nothing installed (+164 MiB).
- On Windows FFmpeg and libvips are fetched by exact URL, version and SHA-256 (FFmpeg n9.0.2, libvips 8.18.7) instead of an unverified "latest release", and the GitHub token is no longer sent to conda.anaconda.org.
- Security: write permission was removed from every CI job except the ones that publish, and tokio 1.54.1 plus rustix 1.1.6 close RUSTSEC-2026-0384 and RUSTSEC-2026-0486.
- About shows the real version from package.json instead of 0.1.0; the macOS key hint names the Accessibility permission instead of blaming Wayland; Escape closes the About and Donate dialogs; the Donate dialog is fully Arabic.
- Package smoke test with system tools hidden: 81 passed, 0 failed, 3 skipped (ExifTool and QR reading stay optional).
- Android 1.3.0 (in the zeidex-convert-android repository): any document becomes a PDF on the device - DOCX, XLSX/ODS/CSV, PPTX, ODT, RTF, HTML, EPUB and Markdown to a PDF with selectable text and drawn tables, plus PDF to DOCX.

Assets: Linux AppImage (x64 and arm64), Windows x64 setup, macOS DMG (Apple Silicon and Intel), each with its digest in `SHA256SUMS-*.txt`.
