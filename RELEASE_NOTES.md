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
