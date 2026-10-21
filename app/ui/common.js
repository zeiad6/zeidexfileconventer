// Shared by every window: i18n (Arabic when the system language is Arabic) and the options form.
(() => {
  const ar = (navigator.language || '').toLowerCase().startsWith('ar');
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
    hookOn: ['Drag any file, then tap Shift (convert), Alt (tools) or Ctrl (actions) and drop it on the wheel.',
      'اسحب أي ملف، ثم اضغط Shift (تحويل) أو Alt (أدوات) أو Ctrl (إجراءات)، وأفلته على العجلة.'],
    hookOff: ['Global keys are not available in this session (Wayland blocks them). Drag files onto the orange orb instead.',
      'المفاتيح العامة غير متاحة في هذه الجلسة (Wayland يمنعها). اسحب الملفات إلى الدائرة البرتقالية بدلاً من ذلك.'],
    nothing: ['Nothing for this selection', 'لا شيء لهذا التحديد'],
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
    saved: ['Saved', 'تم الحفظ'],
  };
  window.ZLANG = ar ? 'ar' : 'en';
  window.ZT = (k) => (S[k] ? S[k][ar ? 1 : 0] : k);
  window.ZL = (o) => (ar && o.label_ar ? o.label_ar : o.label);
  document.documentElement.lang = window.ZLANG;
  document.documentElement.dir = ar ? 'rtl' : 'ltr';
  document.addEventListener('DOMContentLoaded', () => {
    document.querySelectorAll('[data-t]').forEach((el) => { el.textContent = window.ZT(el.dataset.t); });
  });

  /** Fills `form` with inputs for `choice.params`; calls submit(opts) or cancel(). */
  window.ZForm = (form, choice, submit, cancel) => {
    const make = (tag, props) => Object.assign(document.createElement(tag), props);
    form.replaceChildren(make('h3', { textContent: window.ZL(choice) }));
    for (const p of choice.params) {
      const label = make('label', { textContent: window.ZL(p) });
      label.append(make('input', { name: p.key, value: p.default, autocomplete: 'off' }));
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
