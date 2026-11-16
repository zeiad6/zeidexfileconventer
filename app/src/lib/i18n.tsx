/**
 * Bilingual copy (English / Arabic) with instant switching and RTL. The table is the single
 * source of truth: a key that is missing renders as the key, which the unit test catches.
 */

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import type { Choice, Sub } from './types';

export type Lang = 'en' | 'ar';

type Pair = readonly [string, string];

export const COPY = {
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
  hookOn: [
    'Drag any file, then tap Shift (convert), Ctrl (tools) or Alt (actions) and drop it on the wheel. Or select files and press Ctrl+Shift+Space.',
    'اسحب أي ملف، ثم اضغط Shift (تحويل) أو Ctrl (أدوات) أو Alt (إجراءات)، وأفلته على العجلة. أو حدّد ملفات واضغط Ctrl+Shift+Space.',
  ],
  selectionHint: [
    'Ctrl+Shift+Space found no files: select them in Explorer, or copy them (Ctrl+C) in your file manager, then press it again.',
    'لم يجد Ctrl+Shift+Space ملفات: حدّدها في Explorer، أو انسخها (Ctrl+C) في مدير الملفات، ثم اضغط الاختصار مجدداً.',
  ],
  hookOff: [
    'Global keys are not available in this session (Wayland blocks them). Drag files onto the orange orb instead.',
    'المفاتيح العامة غير متاحة في هذه الجلسة (Wayland يمنعها). اسحب الملفات إلى الدائرة البرتقالية بدلاً من ذلك.',
  ],
  nothing: ['Nothing for this selection', 'لا شيء لهذا التحديد'],
  mixed: ['Mixed files: each format applies where it fits', 'ملفات مختلطة: كل صيغة تُطبَّق على ما يدعمها'],
  jobs: ['Jobs', 'المهام'],
  show: ['Show', 'إظهار'],
  clear: ['Clear', 'مسح'],
  scanToReceive: [
    'On the phone: Zeidex Convert → Tools → Receive from computer, then photograph this code.',
    'على الهاتف: Zeidex Convert ← الأدوات ← استلام من الحاسوب، ثم صوّر هذا الرمز.',
  ],
  pairCode: ['LocalDrop pairing code', 'رمز اقتران LocalDrop'],
  sentTo: ['Sent {n} file(s) to {device}', 'أُرسل {n} ملف إلى {device}'],
  declinedBy: ['{device} declined the files', 'رفض {device} الملفات'],
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
  trayIcon: ['Show the tray icon', 'إظهار أيقونة الدرج'],
  saved: ['Saved', 'تم الحفظ'],
  recipes: ['Recipes (shown in the Tools wheel)', 'الوصفات (تظهر في عجلة الأدوات)'],
  recipeName: ['Name', 'الاسم'],
  recipeSteps: ['Steps, e.g. resize max=1600 | compress percent=60', 'الخطوات، مثل resize max=1600 | compress percent=60'],
  watchFolders: ['Watch folders', 'مجلدات مراقَبة'],
  navConvert: ['Converter', 'التحويل'],
  navShelf: ['Floating shelf', 'الرف العائم'],
  navTools: ['Studio tools', 'أدوات الاستوديو'],
  navWatch: ['Watch folders', 'مجلدات مراقَبة'],
  shelfHint: [
    'Files parked here stay put while you gather more from other folders. Drag them out of the orange orb into any app, or open them here as one batch.',
    'الملفات الموجودة هنا تبقى محفوظة أثناء جمع ملفات أخرى من مجلدات مختلفة. اسحبها من الدائرة البرتقالية إلى أي تطبيق، أو افتحها هنا دفعة واحدة.',
  ],
  shelfEmpty: [
    'The shelf is empty. Drop files on the orange orb, or use Choose files.',
    'الرف فارغ. أفلت الملفات على الدائرة البرتقالية، أو استخدم «اختر ملفات».',
  ],
  openAll: ['Open as one batch', 'فتح دفعة واحدة'],
  open: ['Open', 'فتح'],
  watchHint: [
    'Rules run whenever a matching file appears in the folder, even while this window is closed.',
    'تُنفَّذ القواعد عند ظهور ملف مطابق في المجلد، حتى إن كانت هذه النافذة مغلقة.',
  ],
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
  detectPii: ['Detect e-mails & numbers', 'كشف البريد والأرقام'],
  noPii: ['No e-mails or phone/card numbers found.', 'لم يُعثر على بريد أو أرقام هاتف/بطاقات.'],
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
  about: ['About', 'حول البرنامج'],
  aboutTitle: ['About Zeidex Convert', 'حول برنامج Zeidex Convert'],
  developedBy: ['Developed by ZeidexSoft', 'تم تطويره بواسطة ZeidexSoft'],
  developerLink: ['Developer Link', 'رابط المطور'],
  devWebsite: ['https://zeidexlink.zeidexsoft.com/zeidex', 'https://zeidexlink.zeidexsoft.com/zeidex'],
  officialSite: ['Official Website: zeidexsoft.com', 'الموقع الرسمي: zeidexsoft.com'],
  aboutDesc: [
    'A fast, fully offline file conversion and media processing powerhouse. Your files never leave your computer.',
    'منظومة سريعة وشاملة لتحويل ومعالجة الملفات والوسائط محلياً دون أي اتصال بالإنترنت، مع الحفاظ التام على سرية وخصوصية بياناتك.',
  ],
  version: ['Version', 'الإصدار'],
  visitDev: ['Visit Developer Profile', 'زيارة صفحة المطور (ZeidexLink)'],
  visitSite: ['Visit Website', 'زيارة الموقع الرسمي'],
  donate: ['Donate', 'تبرع'],
  donateTitle: ['Support ZeidexSoft Development', 'دعم استمرار تطوير ZeidexSoft'],
  donateDesc: [
    'Zeidex Convert is completely free and privacy-focused. You can support continuous maintenance and new features using any of the available payment methods on zeidexsoft.com below:',
    'برنامج Zeidex Convert مجاني ومفتوح بالكامل. يمكنك دعم استمرار المشروع وإضافة تحسينات مستمرة عبر طرق الدفع المتاحة في موقع zeidexsoft.com أدناه:',
  ],
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
  minimize: ['Minimize', 'تصغير'],
  maximize: ['Maximize', 'تكبير'],
  // rdev reports the same failure on macOS, where the cause is a missing permission, not Wayland.
  hookOffMac: [
    'Global keys need permission: System Settings → Privacy & Security → Accessibility, then enable Zeidex Convert and reopen it. Until then, drag files onto the orange orb.',
    'المفاتيح العامة تحتاج إذناً: إعدادات النظام ← الخصوصية والأمان ← إمكانية الوصول، ثم فعّل Zeidex Convert وأعد فتحه. إلى ذلك الحين اسحب الملفات إلى الدائرة البرتقالية.',
  ],
  payRegion: ['Yemen / Regional', 'اليمن / إقليمي'],
  payInstant: ['Instant Stars', 'نجوم فورية'],
  payChain: ['Crypto / Blockchain', 'عملات رقمية / بلوكتشين'],
  tonLabel: ['TON wallet:', 'محفظة TON:'],
  langSwitch: ['العربية', 'English'],
} satisfies Record<string, Pair>;

export type CopyKey = keyof typeof COPY;

const STORAGE_KEY = 'zeidex_lang';

function initialLang(): Lang {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'ar' || saved === 'en') return saved;
  } catch {
    /* storage can be disabled in a sandboxed window */
  }
  return (navigator.language || '').toLowerCase().startsWith('ar') ? 'ar' : 'en';
}

interface I18n {
  lang: Lang;
  ar: boolean;
  t(key: string): string;
  zl(choice: Pick<Choice | Sub, 'label' | 'label_ar'>): string;
  setLang(lang: Lang): void;
  toggle(): void;
}

const Context = createContext<I18n | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(initialLang);

  useEffect(() => {
    document.documentElement.lang = lang;
    document.documentElement.dir = lang === 'ar' ? 'rtl' : 'ltr';
  }, [lang]);

  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* ignore */
    }
  }, []);

  const value = useMemo<I18n>(() => {
    const ar = lang === 'ar';
    return {
      lang,
      ar,
      t: (key) => {
        const pair = (COPY as Record<string, Pair | undefined>)[key];
        return pair ? pair[ar ? 1 : 0] : key;
      },
      zl: (choice) => (ar && choice.label_ar ? choice.label_ar : choice.label),
      setLang,
      toggle: () => setLang(ar ? 'en' : 'ar'),
    };
  }, [lang, setLang]);

  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useI18n(): I18n {
  const value = useContext(Context);
  if (!value) throw new Error('useI18n must be used inside <I18nProvider>');
  return value;
}
