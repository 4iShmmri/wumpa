import { useStore } from "./store";

export type Lang = "en" | "ar";
export type LangPref = Lang | "auto";

const dict = {
  tipOpen: { en: "Open (⌘O)", ar: "فتح (⌘O)" },
  tipSave: { en: "Save (⌘S)", ar: "حفظ (⌘S)" },
  tipSaveAs: { en: "Save As (⇧⌘S)", ar: "حفظ باسم (⇧⌘S)" },
  btnSaveAs: { en: "Save As", ar: "حفظ باسم" },
  tipPrint: { en: "Print (⌘P)", ar: "طباعة (⌘P)" },
  tipUndo: { en: "Undo (⌘Z)", ar: "تراجع (⌘Z)" },
  tipRedo: { en: "Redo (⇧⌘Z)", ar: "إعادة (⇧⌘Z)" },
  tipZoomOut: { en: "Zoom out (⌘-)", ar: "تصغير (⌘-)" },
  tipZoomIn: { en: "Zoom in (⌘+)", ar: "تكبير (⌘+)" },
  tipFit: { en: "Fit width", ar: "ملاءمة العرض" },
  tipRotL: { en: "Rotate left", ar: "تدوير لليسار" },
  tipRotR: { en: "Rotate right", ar: "تدوير لليمين" },
  tipDelPages: { en: "Delete page(s)", ar: "حذف الصفحات المحددة" },
  tipExtract: { en: "Extract page(s) to a new PDF", ar: "استخراج صفحات إلى ملف جديد" },
  tipMerge: { en: "Merge PDF files", ar: "دمج ملفات PDF" },
  btnMerge: { en: "Merge", ar: "دمج" },
  tipSplit: { en: "Split into multiple files", ar: "تقسيم إلى عدة ملفات" },
  btnSplit: { en: "Split", ar: "تقسيم" },
  tipHighlight: { en: "Highlight selected text", ar: "تمييز النص المحدد" },
  tipNote: { en: "Add note (N)", ar: "إضافة ملاحظة (N)" },
  tipEdit: { en: "Edit objects (E)", ar: "تحرير الكائنات (E)" },
  tipErase: { en: "Delete area (R)", ar: "حذف منطقة (R)" },
  tipText: { en: "Add text (T)", ar: "إضافة نص (T)" },
  tipSign: { en: "Signature (S)", ar: "توقيع (S)" },
  tipDraw: { en: "Draw (D)", ar: "رسم (D)" },
  tipSearch: { en: "Search (⌘F)", ar: "بحث (⌘F)" },
  tipTheme: { en: "Toggle theme", ar: "تبديل المظهر" },
  tipLang: { en: "Language: {lang}", ar: "اللغة: {lang}" },
  langEn: { en: "English", ar: "الإنجليزية" },
  langAr: { en: "Arabic", ar: "العربية" },
  langAuto: { en: "Auto", ar: "تلقائي" },
  tipColor: { en: "Color", ar: "اللون" },
  tipSmaller: { en: "Smaller", ar: "أصغر" },
  tipLarger: { en: "Larger", ar: "أكبر" },
  tipEditText: { en: "Edit text", ar: "تحرير النص" },
  tipDelete: { en: "Delete", ar: "حذف" },
  tipOpenTab: { en: "Open PDF (⌘O)", ar: "فتح PDF (⌘O)" },
  tipPrevMatch: { en: "Previous (⇧Enter)", ar: "السابق (⇧Enter)" },
  tipNextMatch: { en: "Next (Enter)", ar: "التالي (Enter)" },
  tipCloseSearch: { en: "Close (Esc)", ar: "إغلاق (Esc)" },
  tipPrevPage: { en: "Previous page", ar: "الصفحة السابقة" },
  tipNextPage: { en: "Next page", ar: "الصفحة التالية" },
  tipGoToPage: { en: "Go to page", ar: "الانتقال إلى صفحة" },

  kInk: { en: "Freehand", ar: "قلم حر" },
  kRect: { en: "Rectangle", ar: "مستطيل" },
  kEllipse: { en: "Ellipse", ar: "شكل بيضاوي" },
  kLine: { en: "Line", ar: "خط" },
  kArrow: { en: "Arrow", ar: "سهم" },

  wTagline: { en: "Fast, private PDF editing", ar: "تحرير PDF سريع وخاص" },
  wOpen: { en: "Open PDF…", ar: "فتح PDF…" },
  wMerge: { en: "Merge PDF files…", ar: "دمج ملفات PDF…" },
  wRecent: { en: "Recent", ar: "الملفات الأخيرة" },
  wRecentEmpty: { en: "Files you open appear here", ar: "الملفات التي تفتحها تظهر هنا" },
  wHeroTitle: { en: "Drop PDF files anywhere", ar: "أسقط ملفات PDF في أي مكان" },
  wHeroOr: { en: "or press", ar: "أو اضغط" },
  wHeroBrowse: { en: "to browse", ar: "للاستعراض" },
  wHintEdit: { en: "edit objects", ar: "تحرير الكائنات" },
  wHintText: { en: "add text", ar: "إضافة نص" },
  wHintSign: { en: "signature", ar: "توقيع" },
  wHintDraw: { en: "draw", ar: "رسم" },
  wHintSearch: { en: "search", ar: "بحث" },

  splitTitle: { en: "Split document", ar: "تقسيم المستند" },
  splitEvery: { en: "Split every", ar: "تقسيم كل" },
  splitPages: { en: "page(s)", ar: "صفحة" },
  btnCancel: { en: "Cancel", ar: "إلغاء" },
  btnChooseFolderSplit: { en: "Choose folder & split", ar: "اختيار مجلد والتقسيم" },

  sigTitle: { en: "Signature", ar: "التوقيع" },
  sigAlt: { en: "Saved signature", ar: "التوقيع المحفوظ" },
  btnDrawNew: { en: "Draw new", ar: "رسم جديد" },
  btnUse: { en: "Use", ar: "استخدام" },
  btnClear: { en: "Clear", ar: "مسح" },
  btnSaveUse: { en: "Save & use", ar: "حفظ واستخدام" },

  notePlaceholder: { en: "Write a note…", ar: "اكتب ملاحظة…" },
  noteNoText: { en: "(no text)", ar: "(بدون نص)" },
  noteTitle: { en: "Note", ar: "ملاحظة" },
  noteUnsaved: { en: "Note (unsaved)", ar: "ملاحظة (غير محفوظة)" },
  btnDelete: { en: "Delete", ar: "حذف" },
  btnSave: { en: "Save", ar: "حفظ" },
  btnReplace: { en: "Replace", ar: "استبدال" },
  btnEdit: { en: "Edit", ar: "تحرير" },
  btnEditText: { en: "Edit text", ar: "تحرير النص" },

  teLoading: { en: "Loading…", ar: "جارٍ التحميل…" },
  teUnreadable: {
    en: "Original text could not be read — type replacement",
    ar: "تعذرت قراءة النص الأصلي — اكتب النص البديل",
  },
  loading: { en: "Loading…", ar: "جارٍ التحميل…" },

  searchPlaceholder: { en: "Find in document…", ar: "بحث في المستند…" },
  sbOf: { en: "of {total}", ar: "من {total}" },
  sbSelected: { en: "{n} selected", ar: "{n} محددة" },

  ctxDuplicate: { en: "Duplicate page", ar: "تكرار الصفحة" },
  ctxInsertBefore: { en: "Insert blank before", ar: "إدراج صفحة فارغة قبلها" },
  ctxInsertAfter: { en: "Insert blank after", ar: "إدراج صفحة فارغة بعدها" },
  ctxExportPng: { en: "Export as PNG…", ar: "تصدير كـ PNG…" },
  ctxRotL: { en: "Rotate left", ar: "تدوير لليسار" },
  ctxRotR: { en: "Rotate right", ar: "تدوير لليمين" },
  ctxExtract: { en: "Extract page(s)…", ar: "استخراج الصفحات…" },
  ctxDelete: { en: "Delete page(s)", ar: "حذف الصفحات" },

  stSaved: { en: "Saved {name}", ar: "تم حفظ {name}" },
  stSaveFailed: { en: "Save failed", ar: "فشل الحفظ" },
  stCouldNotOpen: { en: "Could not open {name}", ar: "تعذر فتح {name}" },
  stPickTwo: { en: "Pick at least two PDF files to merge", ar: "اختر ملفين PDF على الأقل للدمج" },
  stMerged: { en: "Merged {n} files", ar: "تم دمج {n} ملفات" },
  stMergeFailed: { en: "Merge failed", ar: "فشل الدمج" },
  stExtracted: { en: "Extracted {n} page(s)", ar: "تم استخراج {n} صفحة" },
  stExtractFailed: { en: "Extract failed", ar: "فشل الاستخراج" },
  stSplitDone: { en: "Split into {n} files in {dir}/", ar: "تم التقسيم إلى {n} ملفات في {dir}/" },
  stSplitFailed: { en: "Split failed", ar: "فشل التقسيم" },
  stCannotDeleteAll: { en: "Cannot delete all pages", ar: "لا يمكن حذف كل الصفحات" },
  stExportedPng: { en: "Exported page {n} as PNG", ar: "تم تصدير الصفحة {n} كـ PNG" },
  stPngFailed: { en: "PNG export failed", ar: "فشل تصدير PNG" },
  stPrintOpening: {
    en: "Opening in system viewer for printing…",
    ar: "جارٍ الفتح في عارض النظام للطباعة…",
  },
  stPrintFailed: { en: "Print failed", ar: "فشلت الطباعة" },
  stSelectTextFirst: {
    en: "Select some text first, then click Highlight",
    ar: "حدد نصاً أولاً ثم اضغط زر التمييز",
  },
  stSelectionMap: {
    en: "Could not map the selection to a page",
    ar: "تعذر تحديد موضع التحديد في الصفحة",
  },
  stHighlightAdded: {
    en: "Highlight added (saved into the PDF on save)",
    ar: "تمت إضافة التمييز (يُحفظ داخل الملف عند الحفظ)",
  },
  stSearchFailed: { en: "Search failed", ar: "فشل البحث" },
  stLoadFailed: { en: "Load failed", ar: "فشل التحميل" },
  stObjEditUnavailable: {
    en: "Object editing unavailable on this page",
    ar: "تحرير الكائنات غير متاح في هذه الصفحة",
  },
  stNoObjectsArea: { en: "No objects in that area", ar: "لا توجد كائنات في تلك المنطقة" },
  stReadAnnots: { en: "Could not read annotations", ar: "تعذرت قراءة التعليقات" },
  stCouldNotPlace: { en: "Could not place item", ar: "تعذر وضع العنصر" },
  stPreviewFailed: { en: "Edit preview failed", ar: "فشلت معاينة التعديل" },

  cfCloseUnsaved: {
    en: '"{name}" has unsaved changes. Close without saving?',
    ar: 'يحتوي "{name}" على تغييرات غير محفوظة. إغلاق دون حفظ؟',
  },
  cfQuitUnsaved: {
    en: "{n} file(s) have unsaved changes. Quit anyway?",
    ar: "توجد تغييرات غير محفوظة في {n} من الملفات. الخروج على أي حال؟",
  },
  cfDeleteObjects: { en: "Delete {n} object(s)?", ar: "حذف {n} من الكائنات؟" },

  pwWrong: { en: "Wrong password. Try again:", ar: "كلمة السر غير صحيحة. حاول مجدداً:" },
  pwPrompt: {
    en: '"{name}" is password-protected. Enter password:',
    ar: '"{name}" محمي بكلمة سر. أدخل كلمة السر:',
  },
} satisfies Record<string, { en: string; ar: string }>;

export type MsgKey = keyof typeof dict;

const warned = new Set<string>();

export function t(key: MsgKey, vars?: Record<string, string | number>): string {
  const entry = dict[key] as { en: string; ar: string } | undefined;
  let s: string;
  if (!entry) {
    if (!warned.has(key)) {
      console.warn("i18n: missing key", key);
      warned.add(key);
    }
    s = key;
  } else {
    s = entry[useStore.getState().effectiveLang] ?? entry.en;
  }
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      s = s.split(`{${k}}`).join(String(v));
    }
  }
  return s;
}

export function useT() {
  useStore((s) => s.effectiveLang);
  return t;
}
