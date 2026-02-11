import type { QuickSaveButtonCopy } from './quick-save-engine'

export const STANDARD_QUICK_SAVE_BUTTON_COPY: QuickSaveButtonCopy = {
  idleText: '入库',
  savingText: '入库中',
  doneText: '已入库',
  importedText: '已入库',
  errorText: '失败',
}

export function standardQuickSaveNavigateFallbackToast(): string {
  return '正在打开会话，加载后自动入库...'
}

export const STANDARD_QUICK_SAVE_PILL_STYLE_CSS = `
  .refine-quick-save-host {
    position: relative !important;
    padding-right: 56px !important;
  }

  .refine-quick-save-btn {
    position: absolute;
    top: 50%;
    right: 8px;
    transform: translateY(-50%);
    height: 22px;
    min-width: 38px;
    padding: 0 8px;
    border-radius: 999px;
    border: 1px solid rgba(148, 163, 184, 0.38);
    background: rgba(15, 23, 42, 0.86);
    color: #e2e8f0;
    font-size: 11px;
    line-height: 20px;
    cursor: pointer;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.18s ease, background-color 0.18s ease, border-color 0.18s ease, color 0.18s ease;
    z-index: 2;
  }

  .refine-quick-save-host:hover .refine-quick-save-btn,
  .refine-quick-save-host:focus-within .refine-quick-save-btn,
  .refine-quick-save-btn[data-state="saving"],
  .refine-quick-save-btn[data-state="done"],
  .refine-quick-save-btn[data-state="imported"],
  .refine-quick-save-btn[data-state="error"] {
    opacity: 1;
    pointer-events: auto;
  }

  .refine-quick-save-btn:hover {
    background: rgba(30, 41, 59, 0.92);
    border-color: rgba(148, 163, 184, 0.56);
  }

  .refine-quick-save-btn[data-state="saving"] {
    color: #bfdbfe;
    border-color: rgba(96, 165, 250, 0.56);
  }

  .refine-quick-save-btn[data-state="done"],
  .refine-quick-save-btn[data-state="imported"] {
    color: #86efac;
    border-color: rgba(52, 211, 153, 0.62);
  }

  .refine-quick-save-btn[data-state="error"] {
    color: #fca5a5;
    border-color: rgba(248, 113, 113, 0.62);
  }
`
