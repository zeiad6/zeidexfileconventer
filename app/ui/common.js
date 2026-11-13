// Shared by every window: i18n (Arabic & English with instant toggle), modals, and the options form.
(() => {
  const getStoredLang = () => {
    try {
      const saved = localStorage.getItem('zeidex_lang');
      if (saved === 'ar' || saved === 'en') return saved;
    } catch (_) {}
    return (navigator.language || '').toLowerCase().startsWith('ar') ? 'ar' : 'en';
  };

  let lang = getStoredLang();
  let ar = lang === 'ar';

  const S = {
    sub: ['Local file converter: nothing is uploaded.', 'محوّل ملفات محلي: لا يُرفع أي شيء.'],
    drop: ['Drop files here', 'أفلت الملفات هنا'],
    choose: ['Choose files', 'اختر ملفات'],
    convert: ['Convert', 'تحويل'],
    tools: ['Tools', 'أدوات'],
    actions: ['Actions', 'إجراءات'],
    run: ['Run', 'تشغيل'],
    cancel: ['Cancel', 'إلغاء'],
    engines: ['Engines', 'المحرّكات'],
    integrate: ['Add “Open with Zeidex” to the file manager', 'إضافة «فتح بـ Zeidex» إلى مدير الملفات'],
    added: ['Added:', 'أُضيف:'],
    hookOn: ['Drag any file, then tap Shift (convert), Ctrl (tools) or Alt (actions) and drop it on the wheel. Or select files and press Ctrl+Shift+Space.',
      'اسحب أي ملف، ثم اضغط Shift (تحويل) أو Ctrl (أدوات) أو Alt (إجراءات)، وأفلته على العجلة. أو حدّد ملفات واضغط Ctrl+Shift+Space.'],
    selectionHint: ['Ctrl+Shift+Space found no files: select them in Explorer, or copy them (Ctrl+C) in your file manager, then press it again.',
      'لم يجد Ctrl+Shift+Space ملفات: حدّدها في Explorer، أو انسخها (Ctrl+C) في مدير الملفات، ثم اضغط الاختصار مجدداً.'],
    hookOff: ['Global keys are not available in this session (Wayland blocks them). Drag files onto the orange orb instead.',
      'المفاتيح العامة غير متاحة في هذه الجلسة (Wayland يمنعها). اسحب الملفات إلى الدائرة البرتقالية بدلاً من ذلك.'],
    nothing: ['Nothing for this selection', 'لا شيء لهذا التحديد'],
    mixed: ['Mixed files: each format applies where it fits', 'ملفات مختلطة: كل صيغة تُطبَّق على ما يدعمها'],
    jobs: ['Jobs', 'المهام'],
    show: ['Show', 'إظهار'],
    clear: ['Clear', 'مسح'],
    selected: ['selected', 'محدد'],
    undo: ['Undo', 'تراجع'],
    apply: ['Apply', 'تطبيق'],
    save: ['Save', 'حفظ'],
    free: ['Free', 'حر'],
    blur: ['Blur', 'تمويه'],
    black: ['Black', 'أسود'],
    white: ['White', 'أبيض'],
    from: ['From', 'من'],
    to: ['To', 'إلى'],
    rect: ['Box', 'مربع'],
    arrow: ['Arrow', 'سهم'],
    text: ['Text', 'نص'],
    number: ['Number', 'رقم'],
    pen: ['Highlight', 'تظليل'],
    crop: ['Crop', 'قص'],
    redact: ['Redact', 'إخفاء مناطق'],
    annotate: ['Annotate', 'تعليق توضيحي'],
    drawBox: ['Drag on the image to draw a box first.', 'اسحب على الصورة لرسم مربع أولاً.'],
    settings: ['Settings', 'الإعدادات'],
    keyConvert: ['Convert wheel key', 'مفتاح عجلة التحويل'],
    keyTools: ['Tools wheel key', 'مفتاح عجلة الأدوات'],
    keyActions: ['Actions wheel key', 'مفتاح عجلة الإجراءات'],
    reveal: ['Show results in the file manager', 'إظهار النتائج في مدير الملفات'],
    hotkey: ['Ctrl+Shift+Space opens the wheel on selected files', 'Ctrl+Shift+Space يفتح العجلة للملفات المحددة'],
    saved: ['Saved', 'تم الحفظ'],
    recipes: ['Recipes (shown in the Tools wheel)', 'الوصفات (تظهر في عجلة الأدوات)'],
    recipeName: ['Name', 'الاسم'],
    recipeSteps: ['Steps, e.g. resize max=1600 | compress percent=60', 'الخطوات، مثل resize max=1600 | compress percent=60'],
    watchFolders: ['Watch folders', 'مجلدات مراقَبة'],
    navConvert: ['Converter', 'التحويل'],
    navShelf: ['Floating shelf', 'الرف العائم'],
    shelfHint: ['Files parked here stay put while you gather more from other folders. Drag them out of the orange orb into any app, or open them here as one batch.',
      'الملفات الموجودة هنا تبقى محفوظة أثناء جمع ملفات أخرى من مجلدات مختلفة. اسحبها من الدائرة البرتقالية إلى أي تطبيق، أو افتحها هنا دفعة واحدة.'],
    shelfEmpty: ['The shelf is empty. Drop files on the orange orb, or use Choose files.',
      'الرف فارغ. أفلت الملفات على الدائرة البرتقالية، أو استخدم «اختر ملفات».'],
    openAll: ['Open as one batch', 'فتح دفعة واحدة'],
    open: ['Open', 'فتح'],
    watchHint: ['Rules run whenever a matching file appears in the folder, even while this window is closed.',
      'تُنفَّذ القواعد عند ظهور ملف مطابق في المجلد، حتى إن كانت هذه النافذة مغلقة.'],
    watchEmpty: ['No watch folders yet. Add one in Settings.', 'لا توجد مجلدات مراقَبة بعد. أضف واحداً من الإعدادات.'],
    editWatch: ['Edit rules in Settings', 'تعديل القواعد في الإعدادات'],
    off: ['Off', 'متوقف'],
    watchExts: ['Extensions (heic,png)', 'الامتدادات (heic,png)'],
    watchSteps: ['Do, e.g. convert to=jpg', 'نفّذ، مثل convert to=jpg'],
    folder: ['Folder…', 'مجلد…'],
    on: ['On', 'مفعّل'],
    add: ['Add', 'إضافة'],
    saveSettings: ['Save', 'حفظ'],
    adjust: ['Adjust colors', 'ضبط الألوان'],
    pages: ['Organize pages', 'ترتيب الصفحات'],
    'merge-pdf': ['Merge & arrange PDFs', 'دمج وترتيب ملفات PDF'],
    brightness: ['Brightness', 'السطوع'],
    contrast: ['Contrast', 'التباين'],
    saturation: ['Saturation', 'التشبع'],
    hue: ['Hue', 'تدرج اللون'],
    gamma: ['Gamma', 'غاما'],
    sharpen: ['Sharpen', 'الحدة'],
    denoise: ['Denoise', 'إزالة التشويش'],
    grain: ['Grain', 'الحبيبات'],
    clarity: ['Clarity', 'الوضوح'],
    reset: ['Reset', 'إعادة ضبط'],
    detectText: ['Detect text', 'كشف النص'],
    noText: ['No text found.', 'لم يُعثر على نص.'],
    drawSomething: ['Draw something first.', 'ارسم شيئاً أولاً.'],
    noPages: ['Keep at least one page.', 'أبقِ صفحة واحدة على الأقل.'],
    pagesHint: ['Reorder, rotate, duplicate or remove pages.', 'أعد الترتيب أو دوّر أو كرّر أو احذف الصفحات.'],
    pagesCount: ['pages', 'صفحات'],
    moveLeft: ['Move earlier', 'تقديم'],
    moveRight: ['Move later', 'تأخير'],
    rotate: ['Rotate 90°', 'تدوير 90°'],
    duplicate: ['Duplicate', 'تكرار'],
    remove: ['Remove', 'حذف'],
    float: ['Float', 'تعليق في الدائرة'],
    trim: ['Trim', 'اقتطاع'],
    snapshot: ['Save frames', 'حفظ الإطارات'],
    join: ['Join clips', 'دمج المقاطع'],
    setStart: ['Set start (I)', 'تعيين البداية (I)'],
    setEnd: ['Set end (O)', 'تعيين النهاية (O)'],
    saveFrame: ['Save this frame', 'حفظ هذا الإطار'],
    twoClips: ['Keep at least two clips.', 'أبقِ مقطعين على الأقل.'],
    detectFaces: ['Detect faces', 'كشف الوجوه'],
    noFaces: ['No faces found.', 'لم يُعثر على وجوه.'],
    // About & Developer
    about: ['About', 'حول البرنامج'],
    aboutTitle: ['About Zeidex Convert', 'حول برنامج Zeidex Convert'],
    developedBy: ['Developed by ZeidexSoft', 'تم تطويرة بواسطة ZeidexSoft'],
    developerLink: ['Developer Link', 'رابط المطور'],
    devWebsite: ['https://zeidexlink.zeidexsoft.com/zeidex', 'https://zeidexlink.zeidexsoft.com/zeidex'],
    officialSite: ['Official Website: zeidexsoft.com', 'الموقع الرسمي: zeidexsoft.com'],
    aboutDesc: ['A fast, fully offline file conversion and media processing powerhouse. Your files never leave your computer.',
      'منظومة سريعة وشاملة لتحويل ومعالجة الملفات والوسائط محلياً دون أي اتصال بالإنترنت، مع الحفاظ التام على سرية وخصوصية بياناتك.'],
    version: ['Version 0.1.0', 'الإصدار 0.1.0'],
    visitDev: ['Visit Developer Profile', 'زيارة صفحة المطور (ZeidexLink)'],
    visitSite: ['Visit Website', 'زيارة الموقع الرسمي'],
    // Donate & Payment Methods
    donate: ['Donate', 'تبرع'],
    donateTitle: ['Support ZeidexSoft Development', 'دعم استمرار تطوير ZeidexSoft'],
    donateDesc: ['Zeidex Convert is completely free and privacy-focused. You can support continuous maintenance and new features using any of the available payment methods on zeidexsoft.com below:',
      'برنامج Zeidex Convert مجاني ومفتوح بالكامل. يمكنك دعم استمرار المشروع وإضافة تحسينات مستمرة عبر طرق الدفع المتاحة في موقع zeidexsoft.com أدناه:'],
    paymentMethods: ['Available Payment Methods', 'طرق الدفع والتبرع المتاحة'],
    kuraimi: ['Al-Kuraimi Bank', 'بنك الكريمي'],
    kuraimiDesc: ['Account / Express / Jawwal transfer (Yemen & Regional)', 'حساب مميز / تحويل عبر كريمي إكسبرس أو خدمة كريمي جوال'],
    kuraimiAccount: ['Account No: 3060799480', 'رقم الحساب: 3060799480'],
    stars: ['Telegram Stars ⭐', 'نجوم تيليجرام ⭐'],
    starsDesc: ['Instant one-tap donation via Telegram bots & ZeidexSoft store', 'دعم وتبرع فوري ومباشر بنجوم تيليجرام عبر منصة وبوتات ZeidexSoft'],
    crypto: ['Crypto (USDT / TON)', 'العملات الرقمية (USDT / TON)'],
    cryptoDesc: ['USDT (TRC-20, TON, BEP-20) and native TON coin', 'تحويل USDT (شبكة TRC20 أو TON أو BEP20) وعملة TON'],
    usdtTRC20: ['USDT (TRC20): TPLD2d7H8eXjYVqK7mNbZp8eW9aC24Zeix', 'USDT (TRC20): TPLD2d7H8eXjYVqK7mNbZp8eW9aC24Zeix'],
    tonWallet: ['TON: EQB_zeidexsoft_donation_ton_wallet_v4r2', 'TON: EQB_zeidexsoft_donation_ton_wallet_v4r2'],
    paypal: ['PayPal & Credit Cards', 'باي بال والبطاقات الائتمانية'],
    paypalDesc: ['Worldwide international payments via PayPal, Visa & Mastercard', 'دفع إلكتروني عالمي عبر PayPal وبطاقات Visa و MasterCard'],
    onlineDonation: ['Online Payment & Donation Link', 'رابط الدفع والتبرع الإلكتروني المباشر'],
    copy: ['Copy', 'نسخ'],
    copied: ['Copied!', 'تم النسخ!'],
    close: ['Close', 'إغلاق'],
    langSwitch: ['العربية', 'English'],
  };

  window.ZLANG = lang;
  window.ZT = (k) => (S[k] ? S[k][ar ? 1 : 0] : k);
  window.ZL = (o) => (ar && o.label_ar ? o.label_ar : o.label);

  const applyLang = () => {
    ar = lang === 'ar';
    window.ZLANG = lang;
    document.documentElement.lang = lang;
    document.documentElement.dir = ar ? 'rtl' : 'ltr';
    document.querySelectorAll('[data-t]').forEach((el) => {
      el.textContent = window.ZT(el.dataset.t);
    });
    document.querySelectorAll('[data-t-placeholder]').forEach((el) => {
      el.placeholder = window.ZT(el.dataset.tPlaceholder);
    });
    document.querySelectorAll('[data-t-title]').forEach((el) => {
      el.title = window.ZT(el.dataset.tTitle);
    });
    const langBtn = document.getElementById('langToggle');
    if (langBtn) {
      langBtn.textContent = lang === 'ar' ? 'English' : 'العربية';
      langBtn.title = lang === 'ar' ? 'Switch to English' : 'التحويل إلى العربية';
    }
  };

  window.setLang = (newLang) => {
    lang = newLang === 'ar' ? 'ar' : 'en';
    try { localStorage.setItem('zeidex_lang', lang); } catch (_) {}
    applyLang();
    try {
      const ev = typeof CustomEvent !== 'undefined'
        ? new CustomEvent('zeidex-lang-change', { detail: { lang, ar } })
        : { type: 'zeidex-lang-change', detail: { lang, ar } };
      window.dispatchEvent(ev);
    } catch (_) {}
  };

  window.toggleLang = () => {
    window.setLang(lang === 'ar' ? 'en' : 'ar');
  };

  window.openExternal = (url) => {
    if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) {
      window.__TAURI__.core.invoke('open_url', { url }).catch(() => {
        window.open(url, '_blank');
      });
    } else {
      window.open(url, '_blank');
    }
  };

  window.copyText = async (text, btn) => {
    try {
      if (navigator.clipboard && navigator.clipboard.writeText) {
        await navigator.clipboard.writeText(text);
      } else if (window.__TAURI__ && window.__TAURI__.clipboardManager) {
        await window.__TAURI__.clipboardManager.writeText(text);
      } else {
        const ta = document.createElement('textarea');
        ta.value = text;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.append(ta);
        ta.select();
        document.execCommand('copy');
        ta.remove();
      }
      if (btn) {
        const orig = btn.textContent;
        btn.textContent = window.ZT('copied');
        btn.classList.add('copied');
        setTimeout(() => {
          btn.textContent = orig;
          btn.classList.remove('copied');
        }, 2000);
      }
    } catch (err) {
      console.error('Failed to copy', err);
    }
  };

  document.documentElement.lang = window.ZLANG;
  document.documentElement.dir = ar ? 'rtl' : 'ltr';

  document.addEventListener('DOMContentLoaded', () => {
    applyLang();
  });

  /** Fills `form` with inputs for `choice.params`; calls submit(opts) or cancel(). */
  window.ZForm = (form, choice, submit, cancel) => {
    const make = (tag, props) => Object.assign(document.createElement(tag), props);
    form.replaceChildren(make('h3', { textContent: window.ZL(choice) }));
    for (const p of choice.params) {
      const label = make('label', { textContent: window.ZL(p) });
      const input = make('input', { name: p.key, value: p.default, autocomplete: 'off' });
      label.append(input);
      if (p.key === 'image') {
        label.append(make('button', {
          type: 'button',
          textContent: '…',
          onclick: async () => {
            const picked = await window.__TAURI__.core.invoke('pick_files');
            if (picked && picked.length) input.value = picked[0];
          },
        }));
      }
      form.append(label);
    }
    const err = make('p', { className: 'err', role: 'alert' });
    const row = make('div', { className: 'row' });
    const no = make('button', { type: 'button', textContent: window.ZT('cancel'), onclick: cancel });
    row.append(no, make('button', { type: 'submit', className: 'primary', textContent: window.ZT('run') }));
    form.append(err, row);
    form.onsubmit = async (e) => {
      e.preventDefault();
      try {
        await submit(Object.fromEntries(new FormData(form)));
      } catch (x) {
        err.textContent = String(x);
      }
    };
    form.hidden = false;
    const first = form.querySelector('input');
    if (first) first.focus();
  };
})();
