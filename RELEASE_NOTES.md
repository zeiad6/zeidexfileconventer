## ما الجديد في 0.3.5

- أيقونة جديدة موحّدة لكل الأنظمة: حلقة العجلة بثلاثة قطاعات على مستطيل مستدير داكن، مرسومة مرة واحدة في `icons/icon.svg` ويولّد منها `packaging/make-icons.py` كل مقاسات PNG و`icon.ico` (16–256) و`icon.icns` (بحشو 824/1024 كما يطلب macOS) وشعارات متجر ويندوز وصور مثبّت NSIS. كل مقاس يُرسم من المتجه عند مقاسه بدل تصغير صورة كبيرة، فيبقى 16px حادًّا.
- العمل في الخلفية باستهلاك منخفض للموارد: عند إغلاق واجهة التطبيق يظل البرنامج يعمل في الخلفية (مع خيار في الإعدادات لإيقافه)، وتُخفَّض وتيرة مراقبة السحب في الخمول إلى ~15 Hz (كل 64 ms دون استدعاء موقع المؤشر) وترتفع إلى 60 Hz فقط أثناء السحب أو فتح العجلة. ويمكن فتح النافذة مجددًا عبر تشغيل التطبيق أو من أيقونة الدرج/الكرة العائمة عند تفعيلهما.
- إصلاح ظهور الأدوات والفروع في العجلة الدائرية: الحفاظ على القسم المطلوب (`tools` / `actions`) عند وصول `wheel-mode` قبل تحميل الملفات، وتحديث الحالة بشكل متزامن لمنع إعادة ضبط مؤقتات التبديل والحلقة الفرعية، ودعم ترميز UTF-8 للمسارات العربية/غير اللاتينية وتبويبات Windows 11 Explorer وسطح المكتب في فحص التحديد.
- Tesseract على ويندوز يعمل فعلًا: كان السكربت ينسخ `Library\bin` من حزمة conda وحدها، بينما leptonica وlibtiff وlibarchive وlibcurl وopenjpeg وwebp وpng وlzma وjpeg والـVC runtime في حزم تبعية أخرى، فيصل التنفيذ ناقص الإغلاق ويموت في حُمّال ويندوز بالرمز `0xC0000135` (‎-1073741515) قبل أن يطبع حرفًا. الآن تُحلّ البيئة بـmicromamba ويُنسخ `Library\bin` كاملًا، ويُشغَّل `tesseract.exe --version` و`qrencode.exe --version` كبوابة بناء: إن لم يقلعا يفشل البناء بدل أن يفشل أول OCR عند المستخدم.
- rembg بلا pipx: على ويندوز لا وجود لـpipx أصلًا، فصار زر التثبيت يثبّت Python 3.12 عبر winget ثم `py -3.12 -m pip install --user rembg[cli]`، وأُضيفت مجلدات سكربتات pip–‎user (`%APPDATA%\Python\Python3xx\Scripts`، `~/.local/bin`، `~/Library/Python/3.x/bin`) إلى البحث، وهو سبب عدم عثور التطبيق على rembg بعد تثبيته سابقًا.
- العجلة تعرف ما تسحبه: `CF_HDROP` لا يحمل ملفات سحب OLE إطلاقًا، فصار المسار الاحتياطي تحديدَ الملفات في مدير الملفات ثم `text/uri-list` من الحافظة، على خيط منفصل كي لا يتجمد الـpolling، ويظهر في المركز «Drop files» بدل حلقة فارغة تُقرأ كخطأ.
- تصميم العجلة: قرص بتدرّج وإطارين، النمط الحالي ممتلئ والمُعلَّق عليه إطار فقط، تمييز بغسلة برتقالية تُبقي النص مقروءًا بدل كتلة صلبة، ومركز من سطرين (النمط ثم ما تحت المؤشر)، وإغلاق فرعي بإطار برتقالي بدل الإيموجي. أسماء الأنماط الثلاثة تدور مع حزامها بدل أن تُكتب أفقيًا: كلمة بعرض «Convert» كانت تخرج من حزام سميك 42 وحدة عند الساعة الرابعة وتختفي على القرص الأبيض، والفاصل بين قيم الجودة صار مرئيًا.
- `doctor` لم يعد يضع علامة ✓ على محرّك محزوم لا يقلع: تُجرَّب النسخ المحزومة من Tesseract وlibvips وGhostscript وqrencode مرة واحدة، وتُذكر العلّة («DLL ناقص») مع خطوة الإصلاح.

## What changed in 0.3.5

- One new icon for every platform: the three-segment wheel on a dark squircle, drawn once in `icons/icon.svg` and rendered by `packaging/make-icons.py` into every PNG size, `icon.ico` (16–256), `icon.icns` (padded to Apple's 824/1024 grid) and the Windows Store logos plus the NSIS header and sidebar. Each size is rendered from the vector at its own pixel size instead of downscaling one bitmap, which is what keeps the 16 px copy sharp.
- Low-power background mode: closing the studio window keeps Zeidex running quietly in the background by default (configurable in Settings), with adaptive drag-hook polling that drops to ~15 Hz (64 ms, skipping cursor queries) while idle and switches to 60 Hz only during an active drag or open wheel. Re-launching the app or clicking the optional tray/orb brings the window back immediately.
- Radial wheel tools & branches fix: preserve the requested mode (`tools` / `actions`) when `wheel-mode` arrives before `describe` finishes, synchronize `latest` state updates so 60 Hz hover streams never reset dwell/sub-ring timers, add `wheel_state` recovery on mount, and fix PowerShell UTF-8 encoding (for Arabic/non-ASCII paths), Windows 11 tabbed Explorer, and Desktop selection probing.
- Tesseract actually runs on Windows: the fetch script unzipped the tesseract conda package alone and copied `Library\bin`, while leptonica, libtiff, libarchive, libcurl, openjpeg, webp, png, lzma, jpeg and the VC runtime live in separate dependency packages — so the exe died in the Windows loader with `0xC0000135` (-1073741515) before printing a byte. The environment is now solved with micromamba, its whole `Library\bin` is staged, and `tesseract.exe --version` / `qrencode.exe --version` run as a build gate: a binary that will not start fails the build rather than the user's first OCR.
- rembg without pipx: Windows has no pipx at all, so the Install button now installs Python 3.12 through winget and then runs `py -3.12 -m pip install --user rembg[cli]`. The pip `--user` script folders (`%APPDATA%\Python\Python3xx\Scripts`, `~/.local/bin`, `~/Library/Python/3.x/bin`) were added to the lookup — the reason the app never found rembg after installing it.
- The wheel knows what is being dragged: `CF_HDROP` never carries files during an OLE drag, so the fallback is the file-manager selection and then `text/uri-list` from the clipboard, probed on a worker thread so the 60 Hz poll keeps running. The hub now reads "Drop files" instead of showing an empty ring, which users read as a broken wheel.
- Wheel redesign: a gradient disc with two hairlines, the current mode filled and a dwelled one outlined, hover as an orange wash that keeps 12 px labels readable instead of a solid block, a two-line hub (mode, then the item under the pointer) and an outlined nested ring instead of emoji. The three mode names ride their band's tangent instead of sitting flat: a word as wide as "Convert" left a 42 unit band at 4 o'clock and vanished on the white disc. The quality presets are separated again.
- `doctor` no longer ticks a bundled engine that cannot start: bundled Tesseract, libvips, Ghostscript and qrencode are probed once, and a failure is reported as "the bundled copy does not start (missing DLL)" with the fix beside it.

## ما الجديد في 0.3.4

- ويندوز لم يعد يقول «LibreOffice not found»: LibreOffice 26.2.7 وCalibre 9.18.0 يُضمَّنان في حزمة التثبيت، وتُفكّان بـ`msiexec /a` لأن كابينة الـMSI تحفظ الملفات مسطّحة بلا شجرة `program/` و`share/`. ثم يُقلَّم ما لا يستخدمه التحويل بلا واجهة: صفحات المساعدة وسِمات الأيقونات و56 قاموس تدقيق إملائي (يبقى الإنجليزي والعربي)، وتُنقل خطوط الـMSI إلى `share/fonts/truetype` ليبقى Caladea/Carlito على مقاييس Calibri/Cambria وAmiri للتشكيل العربي. البحث عن المحرّك صار يشمل `LOCALAPPDATA\Programs` و`ProgramW6432` و`ProgramFiles(x86)`، وعلى macOS حزمة `.app`، وعلى لينكس `/usr/lib64` و`/opt/libreoffice*`.
- لأن NSIS لا يبني حزمة يتجاوز فيها مجمع البيانات 2 GB، تُحزَم المحرّكات في `engines.7z` صلب واحد بجانب `7z.exe`، ويفكّه خطاف `NSIS_HOOK_POSTINSTALL` داخل `engines/` أثناء التثبيت؛ وإن فشل 7-Zip يتوقف التثبيت برسالة بدل تطبيق لا يحوّل شيئاً.
- العجلة المنبثقة تعرض الأدوات فعلاً: حدث `wheel-open` كان يفعّل وضع النقر فيمنع أحداث السحب الأصلية، فأُضيف `wheel-files` للسحب، وتُسلَّم مسارات الملفات من الكرة، ويُخفى الوجه إذا كان فارغًا، وتُغلق العجلة بعد 3.2 ثانية خمول مع نبضة `wheel-alive` أثناء السحب البطيء.
- الأدوات التي لا محرّك لها لا تُعرض ثم تفشل: `describe` و`convert` يرشّحان `ocr` و`exif` و`qr-read` و`make-qr` و`transcribe` و`upscale` و`remove-bg` و`merge-pdf`… حسب وجود المحرّك والطراز.
- حزمة ويندوز تحمل أيضًا Tesseract (eng+ara+osd) وqrencode وExifTool وZBar وwhisper.cpp وReal-ESRGAN وطرازَي ggml-tiny وu2net. حزم لينكس وmacOS تحمل ما عدا LibreOffice وCalibre وwhisper على macOS.
- زر «تثبيت» في لوحة Engines يشغّل أمر winget/brew/apt في طرفية مرئية بدل ترك المستخدم أمام رسالة خطأ.
- أداة «إنشاء رمز QR» (qrencode) تعمل من العجلة، وقراءة QR تظهر فقط عند وجود ZBar.

## What changed in 0.3.4

- Windows no longer says `LibreOffice not found`: LibreOffice 26.2.7 and Calibre 9.18.0 travel inside the setup, unpacked with `msiexec /a` because the MSI cabinet stores files flat and has no `program/` + `share/` tree to copy. The staged tree is trimmed of help pages, icon themes and 56 spelling dictionaries (English and Arabic stay), and the installer's `Fonts/` folder is re-homed to `share/fonts/truetype`, so Caladea/Carlito keep Calibri/Cambria metrics and Amiri keeps Arabic shaping. Engine lookup now covers `LOCALAPPDATA\Programs`, `ProgramW6432` and `ProgramFiles(x86)` on Windows, `.app` bundles on macOS, and `/usr/lib64` plus `/opt/libreoffice*` on Linux.
- Because NSIS cannot build an installer whose data block passes 2 GB, the engines travel as one solid `engines.7z` beside the bundled `7z.exe`, and `NSIS_HOOK_POSTINSTALL` unpacks it into `engines/` during setup; a 7-Zip failure aborts the install instead of leaving an app that cannot convert.
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
