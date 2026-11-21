const samples = [
  ["surface-opacity", "65", { windowContent: [".main-content"] }],
  ["settings-section-card", "default", {
    settingsGroup: [".settings-group"], settingsGroupHeader: [".group-header", ".group-header h3"],
    settingsGroupChevron: [".group-header svg"],
  }],
  ["settings-section-card", "expanded", {
    settingsGroupOpen: [".settings-group"], settingsGroupHeaderOpen: [".group-header", ".group-header h3"],
    settingsGroupContent: [".group-content"],
  }],
  ["settings-choices", "default", {
    settingsChoiceCards: [".settings-choice-cards"], settingsChoiceCard: [".settings-choice-card:not(.active)"],
    settingsChoiceCardActive: [".settings-choice-card.active"], settingsSegments: [".segmented-control"],
    settingsSegment: [".segment-btn:not(.active)"], settingsSegmentActive: [".segment-btn.active"],
  }],
  ["settings-choices", "hover-card", { settingsChoiceCardHover: [".settings-choice-card:not(.active)"] }],
  ["settings-choices", "hover-segment", { settingsSegmentHover: [".segment-btn:not(.active)"] }],
  ["emoji-favorites", "default", {
    emojiAddTile: [".emoji-fav-add-btn"], emojiFavoriteTile: [".emoji-fav-card:not(.emoji-fav-add) .emoji-fav-preview"],
    emojiFavoriteRemove: [".emoji-fav-remove"],
  }],
  ["emoji-favorites", "hover", { emojiAddTileHover: [".emoji-fav-add-btn"] }],
  ["settings-data", "default", {
    settingsDataPath: [".data-panel"], settingsDataAction: [".data-actions-row .btn-icon"],
    settingsDataLabel: [".data-path-label"], settingsDataHeader: [".data-path-header"],
    settingsDataBlock: [".showcase-fields .setting-item"], settingsDataActions: [".data-actions-row"],
  }],
  ["settings-data", "hover-path", { settingsDataPathHover: [".data-panel"] }],
  ["settings-data", "hover-action", { settingsDataActionHover: [".data-actions-row .btn-icon"] }],
  ["tag-suggestions", "active", {
    tagSuggestions: [".dc-tag-filter-picker"], tagSuggestionHeading: [".dc-tag-filter-heading"],
    tagSuggestionCount: [".dc-tag-filter-heading small"], tagSuggestionRow: [".dc-tag-filter-row"],
    tagSuggestionOption: [".dc-tag-filter-option:not(.dc-tag-filter-active)"],
    tagSuggestionOptionActive: [".dc-tag-filter-active"], tagSuggestionDot: [".dc-tag-dot"],
  }],
  ["tag-suggestions", "empty", {
    tagSuggestionStatus: [".dc-tag-filter-empty"], tagSuggestionRetry: [".dc-tag-filter-retry"],
  }],
  ["backup-dialog", "default", {
    backupBackdrop: [".data-modal-overlay", null, "::backdrop"],
    backupDialog: [".data-modal-content"], backupTitle: [".data-modal-title"],
    backupField: [".data-form-group"], backupLabel: [".data-label"],
    backupChoices: [".data-radio-group"], backupChoice: [".data-radio-item:has(input:not(:checked))"],
    backupChoiceSelected: [".data-radio-item:has(input:checked)"], backupSubmitDisabled: [".data-btn-primary"],
    backupRadio: [".data-radio-item input:not(:checked)"], backupRadioChecked: [".data-radio-item input:checked"],
    backupInput: [".data-input"], backupPathRow: [".data-path-row"], backupBrowse: [".btn-icon-export-browse"],
  }],
  ["backup-dialog", "encrypted", {
    backupActions: [".data-modal-actions"], backupCancel: [".data-btn-cancel"], backupSubmit: [".data-btn-primary"],
  }],
  ["backup-dialog", "import", {
    backupPreview: [".data-preview-list"], backupPreviewRow: [".data-preview-item"],
  }],
  ["backup-dialog", "replace", {
    backupReplaceConfirmation: [".data-replace-confirm"], backupReplaceMark: [".data-replace-mark"],
  }],
  ["backup-dialog", "replace-confirmed", {
    backupReplaceConfirmationChecked: [".data-replace-confirm"], backupReplaceMarkChecked: [".data-replace-mark"],
  }],
  ["backup-dialog", "replace-hover", { backupReplaceMarkHover: [".data-replace-mark"] }],
  ["backup-dialog", "error", { backupError: [".data-error"] }],
  ["backup-dialog", "focus-password", { backupInputFocus: [".data-input"] }],
  ["backup-dialog", "hover-choice", { backupChoiceHover: [".data-radio-item:nth-child(2)"] }],
  ["backup-dialog", "hover-selected-choice", { backupChoiceSelectedHover: [".data-radio-item:has(input:checked)"] }],
  ["backup-dialog", "hover-browse", { backupBrowseHover: [".btn-icon-export-browse"] }],
  ["backup-dialog", "hover-cancel", { backupCancelHover: [".data-btn-cancel"] }],
  ["backup-dialog", "hover-submit", { backupSubmitHover: [".data-btn-primary"] }],
  ["settings-update", "installing", {
    updateVersion: [".update-version-row"], updateVersionValue: [".update-version-row strong"],
    updateProgressText: [".settings-range-header"], updateProgressTrack: [".update-progress"],
    updateProgressFill: [".update-progress-fill"], updateProgressVerified: [".update-verified"],
  }],
  ["settings-update-actions", "default", { updateActions: [".update-actions"], updateAction: [".update-actions button"] }],
  ["settings-update-actions", "hover", { updateActionHover: [".update-actions button"] }],
  ["settings-update-actions", "busy", { updateActionDisabled: [".update-actions button"] }],
  ["hotkey-list", "multiple", {
    hotkeyEditor: [".hotkey-list-editor"],
    hotkeyEditorHeader: [".hotkey-list-editor > .hotkey-recorder", ".hotkey-list-editor .item-label"],
    hotkeyRemove: [".hotkey-list .btn-icon"],
  }],
  ["settings-remove", "default", {
    settingsRemove: [".settings-remove:not(.settings-remove-icon)"],
    settingsRemoveIcon: [".settings-remove-icon"],
  }],
  ["privacy-rules", "default", {
    privacyRulesHeading: [".privacy-rule-manager .settings-section-title"],
    privacyRulesHeadingLabel: [".privacy-rule-manager .settings-section-title .item-label"],
    privacyRuleAction: [".privacy-rule-card > .settings-action"],
    privacyRuleCancel: [".privacy-rule-confirm .settings-action:not(.settings-remove)"],
    privacyRuleCard: [".privacy-rule-card"],
    privacyRuleTitle: [".privacy-rule-heading .item-label"],
    privacyRuleSource: [".privacy-rule-source"], privacyRuleScope: [".privacy-rule-scope"],
    privacySourceExclusion: [".source-exclusion-input"],
  }],
  ["privacy-rules", "focus-exclusions", {
    privacySourceExclusionFocus: [".source-exclusion-input"],
  }],
  ["privacy-detection", "default", {
    privacyDetectionOptions: [".privacy-detection-options"],
    privacyDetectionOption: [".privacy-kind-option"], privacyDetectionLabel: [".privacy-kind-label"],
    privacyPatternsContainer: [".privacy-patterns-field"],
    privacyPatternsField: [".privacy-patterns-input"],
  }],
  ["privacy-detection", "focus", {
    privacyPatternsFieldFocus: [".privacy-patterns-input"],
  }],
  ["tag-toolbar", "default", {
    tagToolbar: [".tag-manager-toolbar"], tagToolbarButton: [".tag-manager-toolbar .btn-icon:not(.active)"],
    tagToolbarButtonActive: [".tag-manager-toolbar .btn-icon.active"],
    tagToolbarTitle: [".tag-manager-current > span"], tagToolbarTitleIcon: [".tag-manager-current > svg"],
  }],
  ["tag-history-entry", "default", {
    tagHistoryEntry: [".tag-history-row"], tagHistorySelection: [".tag-history-selection:not(:checked)"],
    tagHistorySelectionChecked: [".tag-history-selection:checked"],
    historyMetaWithoutActions: [".tag-history-row .history-item .item-meta"],
  }],
  ["tag-batch-toolbar", "selected", {
    tagBatchToolbar: [".tag-manager-batch-toolbar"], tagBatchSelect: [".tag-manager-batch-toolbar > button:first-child"],
    tagBatchCount: [".tag-manager-batch-toolbar > span"], tagBatchDelete: [".tag-manager-batch-delete"],
  }],
  ["tag-search", "default", {
    tagPickerTrigger: [".tag-manager-picker-trigger", ".tag-manager-picker-trigger > span"],
    tagSearchInput: [".tag-manager-search .search-input"],
    tagSearchInputPlaceholder: [".tag-manager-search .search-input", null, "::placeholder"],
    tagSearchInputIcon: [".tag-manager-search .search-icon"],
    tagSearchClear: [".tag-manager-clear"], tagSearchClearIcon: [".tag-manager-clear svg"],
    tagRailSearchInput: [".tag-manager-rail-search .search-input"],
    tagRailSearchInputPlaceholder: [".tag-manager-rail-search .search-input", null, "::placeholder"],
    tagRailSearchInputIcon: [".tag-manager-rail-search .search-icon"],
    tagPickerSearchInput: [".tag-manager-picker-search .search-input"],
    tagPickerSearchInputPlaceholder: [".tag-manager-picker-search .search-input", null, "::placeholder"],
    tagPickerSearchInputIcon: [".tag-manager-picker-search .search-icon"],
  }],
  ["text-fields", "default", {
    textField: ["input.deziro-text-field:not(.numeric-input)"], numberField: [".numeric-input"],
  }],
  ["text-fields", "focus-text", { textFieldFocus: ["input.deziro-text-field:not(.numeric-input)"] }],
  ["text-fields", "focus-number", { numberFieldFocus: [".numeric-input"] }],
  ["context-menu", "privacy", {
    contextMenu: [".item-context-menu"], contextSubmenu: [".item-context-menu__submenu"],
    contextMenuItem: [".item-context-menu__item:not(.item-context-menu__item--active)"],
    contextMenuItemActive: [".item-context-menu__item--active"],
    contextSubmenuItem: [".item-context-menu__submenu-item:not(.item-context-menu__submenu-item--active)"],
    contextSubmenuItemActive: [".item-context-menu__submenu-item--active"],
  }],
  ["attachment-preview", "media", {
    attachmentName: [".attachment-tile figcaption"], attachmentIcon: [".attachment-tile figcaption svg"],
  }],
  ["attachment-preview", "compact", { attachmentNameCompact: [".attachment-tile figcaption"] }],
  ["history-image", "default", { attachmentImage: [".image-preview"] }],
  ["history-image", "compact", { attachmentImageCompact: [".image-preview"] }],
  ["history-image", "pending", { attachmentImagePending: [".image-preview-pending"] }],
  ["history-image", "compact-pending", { attachmentImageCompactPending: [".image-preview-pending"] }],
  ["history-image", "unavailable", { attachmentImageUnavailable: [".image-preview-unavailable"] }],
  ["history-image", "compact-unavailable", { attachmentImageCompactUnavailable: [".image-preview-unavailable"] }],
  ["history-image", "missing", { attachmentImageMissing: [".history-image-missing"] }],
  ["history-image", "compact-missing", { attachmentImageCompactMissing: [".history-image-missing"] }],
  ["preview-toolbar", "default", {
    previewToolbar: [".preview-toolbar"], previewToolbarTitle: [".preview-toolbar-drag span"],
    previewToolbarButton: [".preview-toolbar-actions .preview-toolbar-button"],
    previewOpacityTrack: [".preview-toolbar-opacity [data-slider-sample=track]"],
    previewOpacityThumb: [".preview-toolbar-opacity [data-slider-sample=thumb]"],
  }],
  ["preview-toolbar", "pinned", {
    previewToolbarActive: [".preview-toolbar-button.active"], previewToolbarActual: [".preview-toolbar-actual"],
  }],
  ["settings-rail", "default", {
    settingsRail: [".settings-rail"], settingsRailItem: [".settings-rail-item:not(.is-active)"],
    settingsRailActive: [".settings-rail-item.is-active"],
  }],
  ["settings-footer", "default", {
    settingsFooterCard: [".settings-footer-card", ".settings-footer-card-label"],
    settingsFooterVersion: [".settings-footer-version"], settingsFooterLink: [".settings-footer-link"],
  }],
  ["scroll-to-top", "default", { scrollToTop: [".scroll-top-button"] }],
  ["scroll-to-top", "hover", { scrollToTopHover: [".scroll-top-button"] }],
  ["settings-toggle", "default", {
    settingsLabel: [".setting-item .item-label"], settingsToggleRow: [".setting-item"],
    settingsSwitch: [".switch"], settingsSwitchTrack: [".toggle"], settingsSwitchThumb: [".toggle", null, "::before"],
    settingsSwitchLeft: [".toggle > .left"], settingsSwitchRight: [".toggle > .right"],
  }],
  ["settings-toggle", "enabled", {
    settingsSwitchTrackOn: [".toggle"], settingsSwitchThumbOn: [".toggle", null, "::before"],
    settingsSwitchLeftOn: [".toggle > .left"], settingsSwitchRightOn: [".toggle > .right"],
  }],
  ["settings-slider", "keyboard", { settingsRangeFocus: [".settings-view input[type=range]"] }],
  ["settings-slider", "default", {
    settingsRangeTrack: ["[data-slider-sample=track]"], settingsRangeThumb: ["[data-slider-sample=thumb]"],
    settingsRangeFill: ["[data-slider-sample=fill]"],
  }],
  ["hotkey-binding", "default", {
    hotkeyRow: [".hotkey-recorder"], hotkeyGroup: [".key-group"], hotkeyCap: [".key-cap"],
  }],
  ["hotkey-binding", "recording", { hotkeyCapRecording: [".key-cap"] }],
  ["character-search", "default", {
    characterInput: [".emoji-search .search-input"],
    characterInputPlaceholder: [".emoji-search .search-input", null, "::placeholder"],
    characterInputIcon: [".emoji-search .search-icon"],
  }],
  ["emoji-tabs", "default", {
    emojiTab: [".emoji-tab:not(.active)"], emojiTabActive: [".emoji-tab.active"], emojiTabs: [".emoji-tabs"],
  }],
  ["emoji-grid", "default", {
    emojiContent: [".emoji-content"], emojiGroup: [".emoji-group"], emojiGroupTitle: [".emoji-group-title"],
    emojiTile: [".emoji-btn"], emojiGrid: [".emoji-grid"],
  }],
  ["history-card", "default", {
    historyCard: [".history-item"], historyMeta: [".item-meta"], historySource: [".item-meta-left .app-info"],
    historyTime: [".item-meta .app-info:last-child span"], historyContent: [".history-item .content-preview"],
    historyActions: [".item-actions"], historyAction: [".item-actions .btn-icon"],
    historyActionIcon: [".item-actions .btn-icon svg"],
  }],
  ["history-tags", "default", {
    historyTags: [".item-tags-container"], historyTag: [".tag-chip"],
  }],
  ["history-tags", "compact", {
    historyTagsCompact: [".item-tags-container"], historyTagCompact: [".tag-chip"],
  }],
  ["history-card", "selected", { historyCardSelected: [".history-item.selected"] }],
  ["history-typography", "default", { historyConcealed: [".privacy-hidden-content"] }],
  ["history-rich-text", "default", { historyRichContent: [".history-rich-preview"] }],
  ["history-rich-text", "compact", { historyRichContentCompact: [".history-rich-preview"] }],
  ["history-ocr", "expanded", {
    historyOCRBadgeDone: [".ocr-badge"], historyOCRSection: [".ocr-text-section"],
    historyOCRButton: [".ocr-text-toggle"], historyOCRContent: [".ocr-text-content"],
  }],
  ["history-ocr", "processing", { historyOCRBadgeProcessing: [".ocr-badge"] }],
  ["history-ocr", "failed", { historyOCRBadgeFailed: [".ocr-badge"] }],
  ["history-ocr", "unsupported", { historyOCRBadgeUnsupported: [".ocr-badge"] }],
  ["history-card", "hover", { historyCardHover: [".history-item"] }],
  ["history-card", "compact", {
    historyCardCompact: [".history-item.compact"], historyContentCompact: [".content-preview"],
    historyMetaCompact: [".item-meta"],
    historyActionsCompactAnchor: [".history-card-meta-right"], historyActionsCompact: [".item-actions"],
    historyActionCompact: [".item-actions .btn-icon"], historyActionCompactIcon: [".item-actions .btn-icon svg"],
  }],
  ["history-card", "compact-hover", { historyCardCompactHover: [".history-item.compact"] }],
  ["history-card", "compact-action-hover", { historyActionCompactHover: [".item-actions .btn-icon"] }],
  ["history-card", "compact-active", { historyActionCompactActive: [".item-actions .btn-icon.active"] }],
  ["history-card", "compact-active-hover", { historyActionCompactActiveHover: [".item-actions .btn-icon.active"] }],
  ["app-header", "default", {
    header: ["header"], headerTop: [".header-top"], headerTitle: [".header-title"],
    headerButton: [".header-top .btn-icon"],
  }],
  ["clipboard-search", "default", {
    headerInput: [".search-container .search-input"],
    headerInputPlaceholder: [".search-container .search-input", null, "::placeholder"],
    headerInputIcon: [".search-container .search-icon"],
    headerSearch: [".search-container"], headerFilters: [".clipboard-search-types"],
    headerFilter: [".clipboard-search-types .btn-icon"],
  }],
  ["clipboard-search", "selected", { headerFilterSelected: [".clipboard-search-types .btn-icon.active"] }],
  ["app-header", "pinned", { headerButtonActive: [".header-top .btn-icon.active"] }],
  ["select", "default", {
    select: [".dezirclip-select__control", ".dezirclip-select__single-value"],
    selectIndicator: [".dezirclip-select__dropdown-indicator"],
  }],
  ["select", "hover", { selectHover: [".dezirclip-select__control", ".dezirclip-select__single-value"] }],
  ["select", "focus", { selectFocus: [".dezirclip-select__control", ".dezirclip-select__single-value"] }],
  ["select", "disabled", { selectDisabled: [".dezirclip-select__control", ".dezirclip-select__single-value"] }],
  ["select", "popup", {
    selectMenu: [".dezirclip-select__menu"],
    selectMenuList: [".dezirclip-select__menu-list"],
    selectOption: [".dezirclip-select__option:nth-child(2)"],
    selectSelected: [".dezirclip-select__option--is-selected"],
    selectOptionDisabled: [".dezirclip-select__option--is-disabled"],
  }],
  ["search-select", "apps-filtered", {
    selectSearchQuery: [".dezirclip-select__input"],
    selectOptionFocused: [".dezirclip-select__option--is-focused"],
  }],
  ["filters", "default", {
    filter: [".filter-chip:nth-child(3)"],
    filterSelected: [".filter-chip--active"],
    filterAction: [".filter-chip--action"],
  }],
  ["search-bar", "default", {
    searchInput: [".search-bar__input"], searchIcon: [".search-bar__icon"],
    searchModes: [".search-bar__mode"],
    searchMode: [".search-bar__mode-btn:nth-child(2)"],
    searchModeSelected: [".search-bar__mode-btn--active"],
    searchModeIcon: [".search-bar__mode-icon"],
    searchModeLabel: [".search-bar__mode-label"],
  }],
  ["search-bar", "loading", { searchSpinner: [".search-bar__spinner"] }],
  ["search-bar", "error", { searchError: [".search-bar__error"] }],
  ["search-bar", "filled", { searchCount: [".search-bar__count"], searchClear: [".search-bar__clear"] }],
  ["highlight", "default", { mark: [".match-highlight mark"], match: [".match-highlight"] }],
  ["search-history", "default", { history: [".search-history"], historyItem: [".search-history__item"], historyIcon: [".search-history__icon"] }],
  ["search-history", "empty", { historyEmpty: [".search-history__empty"] }],
  ["disclosure", "default", {
    disclosure: [".settings-disclosure-toggle", ".settings-disclosure-toggle .item-label"],
    disclosureChevron: [".settings-disclosure-chevron"],
  }],
  ["disclosure", "expanded", { disclosureContent: [".settings-disclosure > div:not([hidden])"] }],
  ["help", "default", { help: [".setting-help-button"] }],
  ["help", "expanded", { helpText: [".setting-help-text"], helpActive: [".setting-help-button"] }],
  ["settings-toggle-help", "expanded", {
    settingsFieldHelpText: [".setting-item .setting-help-text"],
    settingsSwitchDisabled: [".switch input:disabled + .toggle"],
  }],
  ["progress", "default", {
    progress: [".progress-toast"], progressMessage: [".progress-toast-message"],
    progressLabel: [".progress-toast-percent"], progressTrack: [".progress-toast-bar-track"],
    progressFill: [".progress-toast-bar-fill"],
  }],
  ["progress", "error", { progressError: [".progress-toast--error"], progressClose: [".progress-toast-close"] }],
  ["progress", "done", { progressDone: [".progress-toast--done"], progressStatus: [".progress-toast-status"] }],
  ["toast", "default", { toast: [".toast-item"] }],
  ["confirm", "default", {
    confirm: [".confirm-dialog"], confirmBackdrop: [".modal-overlay"],
    confirmTitle: [".confirm-dialog-title"], confirmMessage: [".confirm-dialog-message"],
    confirmCancel: [".confirm-dialog-button:not(.primary)"], confirmPrimary: [".confirm-dialog-button.primary"],
  }],
  ["tag-dialog", "default", {
    tag: [".tag-manager-dialog"], tagBackdrop: [".tag-manager-dialog", null, "::backdrop"],
    tagTitle: [".tag-manager-dialog .confirm-dialog-title"], tagClose: [".tag-manager-dialog .btn-icon"],
    tagHeading: [".tag-manager-dialog-heading"], tagCloseIcon: [".tag-manager-dialog-heading svg"],
    tagField: [".tag-manager-field"], tagFieldLabel: [".tag-manager-field span"], tagInput: [".tag-manager-field input"],
  }],
];

// Chromium returns the input's style for its internal range pseudo-elements.
// Apply those same CSS declarations to offscreen boxes to capture their geometry and paint.
function prepareSliderSamples(selector) {
  const field = document.querySelector(selector);
  const parts = { track: "runnable-track", thumb: "thumb" };
  const sampleStyle = document.createElement("style");
  const activeRules = function* (rules) {
    for (const rule of rules) {
      if (rule instanceof CSSStyleRule) yield rule;
      else if (rule instanceof CSSMediaRule && matchMedia(rule.conditionText).matches ||
               rule instanceof CSSSupportsRule && CSS.supports(rule.conditionText)) yield* activeRules(rule.cssRules);
    }
  };
  const declarations = [];
  for (const sheet of document.styleSheets) {
    for (const rule of activeRules(sheet.cssRules)) {
      for (const [part, suffix] of Object.entries(parts)) {
        const rangePart = new RegExp('input\\[type="range"\\]::-webkit-slider-' + suffix + '$');
        if (!rangePart.test(rule.selectorText)) continue;
        declarations.push(rule.selectorText.replace(rangePart, '[data-slider-sample=' + part + ']') + '{' + rule.style.cssText + '}');
      }
    }
  }
  sampleStyle.textContent = declarations.join("\n");
  document.head.append(sampleStyle);
  const append = part => {
    const sample = document.createElement("div");
    sample.dataset.sliderSample = part;
    sample.style.cssText = "position: absolute; visibility: hidden; pointer-events: none; box-sizing: border-box";
    field.after(sample);
    return sample;
  };
  const track = append("track");
  append("thumb");
  append("fill").style.backgroundColor = "var(--accent-color)";
  const trackStyle = getComputedStyle(track);
  if (trackStyle.backgroundColor === "rgba(0, 0, 0, 0)" && trackStyle.backgroundImage === "none") {
    const fieldStyle = getComputedStyle(field);
    for (const key of ["backgroundColor", "border", "borderRadius", "boxShadow"]) track.style[key] = fieldStyle[key];
  }
}

function readStyles(sample) {
  const selectors = sample.selectors ?? sample;
  const drawing = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
  const colorCache = new Map();
  const color = (...values) => {
    const key = values.join("\n");
    if (colorCache.has(key)) return colorCache.get(key);
    drawing.clearRect(0, 0, 1, 1);
    for (const value of values) {
      if (!CSS.supports("color", value)) continue;
      drawing.fillStyle = value;
      drawing.fillRect(0, 0, 1, 1);
    }
    const rgba = [...drawing.getImageData(0, 0, 1, 1).data];
    colorCache.set(key, rgba);
    return rgba;
  };
  const number = value => Math.round((Number.parseFloat(value) || 0) * 100) / 100;
  const styles = Object.fromEntries(Object.entries(selectors).map(([id, [selector, label, pseudo]]) => {
    const element = document.querySelector(selector);
    if (!element) throw Error("Missing fixture element " + selector);
    const style = getComputedStyle(element, pseudo);
    const text = label ? getComputedStyle(document.querySelector(label)) : style;
    const historyContent = id === "historyContent" || id === "historyContentCompact";
    let usesMonoFont = false;
    if (historyContent) {
      const roots = [document.documentElement.style, document.body.style];
      const saved = roots.map(root => [root.getPropertyValue("--font-mono"), root.getPropertyPriority("--font-mono")]);
      try {
        for (const root of roots) root.setProperty("--font-mono", '"Deziro Font Probe"', "important");
        usesMonoFont = getComputedStyle(element).fontFamily.includes("Deziro Font Probe");
      } finally {
        roots.forEach((root, index) => {
          const [value, priority] = saved[index];
          if (value) root.setProperty("--font-mono", value, priority);
          else root.removeProperty("--font-mono");
        });
      }
    }

    const shadows = style.boxShadow.split(/,(?![^()]*\))/).flatMap(layer => {
      const tint = layer.match(/(?:rgba?|color|oklch)\([^)]*\)/)?.[0];
      if (!tint) return [];
      const values = layer.replace(tint, "").replace(/\binset\b/, "").trim().split(/\s+/).slice(0, 4).map(number);
      return [{ color: color(tint), values, ...(/\binset\b/.test(layer) ? { inset: true } : {}) }];
    }).reverse();
    const sides = ["Top", "Right", "Bottom", "Left"];
    const borderWidths = sides.map(side => number(style["border" + side + "Width"]));
    const borderSide = sides[borderWidths.findIndex(width => width > 0)] ?? "Top";
    const outlineWidth = style.outlineStyle === "solid" ? number(style.outlineWidth) : 0;
    const backgroundImage = style.backgroundImage.replace(/(?:, none)+$/, "");
    const gradient = backgroundImage.match(/^linear-gradient\(([\d.]+)deg, (rgba?\([^)]*\)) 0%, (rgba?\([^)]*\)) 100%\)$/)
      ?? (id.startsWith("settingsSwitch") && backgroundImage.match(
        /^linear-gradient\((?:([\d.]+)deg, )?((?:rgba?|color|oklch)\([^)]*\))(?: 0%)?, ((?:rgba?|color|oklch)\([^)]*\))(?: 100%)?\)$/));
    // Some glass surfaces layer a uniform tint over their background color.
    const tint = backgroundImage.match(/^linear-gradient\((rgba?\([^)]*\)), \1\)$/)?.[1];
    const letterSpacing = number(text.letterSpacing);
    const fontSize = number(text.fontSize);
    const lineHeight = fontSize && number(text.lineHeight) / fontSize;
    const columnGap = number(style.columnGap);
    const rowGap = number(style.rowGap);
    const top = number(style.top);
    const left = number(style.left);
    const right = number(style.right);
    const bottom = number(style.bottom);
    const transform = style.transform !== "none" ? new DOMMatrixReadOnly(style.transform) : null;
    const translateX = number(transform?.m41);
    const translateY = number(transform?.m42);
    const headingShift = id.startsWith("settingsGroupHeader")
      ? (number(text.paddingTop) - number(text.paddingBottom)) / 2 : 0;
    const marginBottom = number(style.marginBottom);
    const backdropBlur = id === "backupDialog" ? number(style.backdropFilter.match(/blur\(([^)]+)\)/)?.[1]) : 0;
    const bounds = element.getBoundingClientRect();
    const switchFace = /^settingsSwitch(Left|Right)/.test(id);
    const switchBounds = switchFace ? element.parentElement.getBoundingClientRect() : null;
    const content = switchFace ? getComputedStyle(element, "::before").content : "";
    return [id, {
      fill: color(style.backgroundColor, id === "settingsRangeTrack" ? "" : tint), text: color(text.color), border: color(style["border" + borderSide + "Color"]),
      borderWidth: borderWidths[0],
      ...(backdropBlur ? { backdropBlur } : {}),
      radius: id === "settingsRangeThumb" && style.borderTopLeftRadius.endsWith("%")
        ? Math.min(bounds.width, bounds.height) * number(style.borderTopLeftRadius) / 100
        : number(style.borderTopLeftRadius),
      ...(style["border" + borderSide + "Style"] === "dashed" ? { dashed: true } : {}),
      ...(borderWidths.some(width => width !== borderWidths[0]) ? { borderWidths } : {}),
      ...(outlineWidth > 0 && (id === "settingsRangeFocus" || number(style.outlineOffset) === -outlineWidth)
        ? { outline: { width: outlineWidth, color: color(style.outlineColor),
            ...(id === "settingsRangeFocus" ? { offset: number(style.outlineOffset) } : {}) } } : {}),
      ...(gradient ? { gradient: { angle: Number(gradient[1] ?? 180), from: color(gradient[2]), to: color(gradient[3]) } } : {}),
      ...(letterSpacing ? { letterSpacing } : {}),
      ...(lineHeight ? { lineHeight: Math.round(lineHeight * 1000) / 1000 } : {}),
      ...(id.startsWith("attachmentImage") ? {
        minWidth: number(style.minWidth), minHeight: number(style.minHeight), maxHeight: number(style.maxHeight),
      } : {}),
      ...(historyContent ? {
        minHeightEm: Math.round(number(style.minHeight) / fontSize * 1000) / 1000,
        maxLines: number(style.webkitLineClamp),
        ...(number(style.maxHeight) ? { maxHeight: number(style.maxHeight) } : {}),
        ...(usesMonoFont ? { usesMonoFont } : {}),
      } : {}),
      ...(id.startsWith("historyRichContent") ? { maxHeight: number(style.maxHeight) } : {}),
      ...(id === "historyOCRContent" ? { maxHeight: number(style.maxHeight), usesMonoFont: true } : {}),
      ...(/^(backupInput|settingsDataPath)(Focus|Hover)?$/.test(id) ? { usesMonoFont: true } : {}),
      ...(id.startsWith("backupReplaceConfirmation") || id.startsWith("privacyPatternsField") ? { minHeight: number(style.minHeight) } : {}),
      ...(columnGap ? { columnGap } : {}),
      ...(rowGap ? { rowGap } : {}),
      ...(switchFace ? { top: number(bounds.top - switchBounds.top), left: number(bounds.left - switchBounds.left) } : top ? { top } : {}),
      ...(id === "historyActionsCompactAnchor" ? { right } : {}),
      ...(id === "historyTagsCompact" ? {
        right, bottom, maxWidthPercent: style.maxWidth.endsWith("%") ? number(style.maxWidth) : 0,
      } : {}),
      ...(/^(historyTag|historyTagCompact)$/.test(id) ? { minHeight: number(style.minHeight) } : {}),
      ...((/^historyActionCompact/.test(id) || /^(previewToolbarActive|tagToolbarButtonActive)$/.test(id)) && translateX ? { translateX } : {}),
      ...((/^historyActions?Compact/.test(id) || /^(previewToolbarActive|tagToolbarButtonActive)$/.test(id)) && translateY ? { translateY } : {}),
      ...(headingShift ? { translateY: headingShift } : {}),
      ...(id.startsWith("settingsSwitchThumb") ? { left: number(left + translateX) } : {}),
      ...(id.endsWith("InputIcon") ? { left } : {}),
      ...(content.startsWith('"') ? { caption: JSON.parse(content) } : {}),
      ...(marginBottom ? { marginBottom } : {}),
      ...(id.startsWith("historyCard") || id.startsWith("hotkey") || id === "settingsDataActions" ||
        id === "privacyDetectionOptions"
        ? { margin: sides.map(side => number(style["margin" + side])) } : {}),
      width: switchFace ? number(bounds.width) : pseudo && pseudo !== "::backdrop" ? number(style.width) : element.offsetWidth ?? bounds.width,
      height: id === "backupPreview" ? number(style.maxHeight)
        : switchFace ? number(bounds.height) : pseudo && pseudo !== "::backdrop" ? number(style.height) : element.offsetHeight ?? bounds.height,
      marginTop: number(style.marginTop),
      padding: [style.paddingTop, style.paddingRight, style.paddingBottom, style.paddingLeft].map(number),
      font: text.fontFamily.split(",").map(family => family.trim().replace(/^['"]|['"]$/g, "")).join(", "),
      size: fontSize, weight: number(text.fontWeight), uppercase: text.textTransform === "uppercase",
      opacity: number(style.opacity),
      ...(shadows.length ? { shadows } : {}),
    }];
  }));
  if (!sample.materialOpacity) return styles;
  const root = document.documentElement.style;
  const originalOpacity = root.getPropertyValue("--surface-opacity");
  const opacityPriority = root.getPropertyPriority("--surface-opacity");
  // Sample finalized colors even for controls with theme-specific transitions.
  const targets = [...new Set(Object.values(selectors).map(([selector]) => document.querySelector(selector)))];
  const transitions = targets.map(element => [
    element, element.style.getPropertyValue("transition"), element.style.getPropertyPriority("transition"),
  ]);
  for (const [element] of transitions) element.style.setProperty("transition", "none", "important");
  const materials = Object.entries(selectors).map(([id, [selector, , pseudo]]) => {
    const element = document.querySelector(selector);
    const css = getComputedStyle(element, pseudo);
    const border = ["Top", "Right", "Bottom", "Left"].find(side => number(css["border" + side + "Width"]) > 0) ?? "Top";
    return { id, css, border, fills: [], borders: [] };
  });
  function materialRange(colors) {
    const equalColor = (a, b) => a.every((channel, i) => channel === b[i]);
    if (colors.every(rgba => equalColor(rgba, colors[0]))) return undefined;
    const stops = [];
    // Compare premultiplied colors so round-off in nearly clear RGB values
    // does not create extra stops. Preserve alpha separately for composition.
    const distance = (a, b) => Math.max(Math.abs(a[3] - b[3]), ...a.slice(0, 3).map((_, i) =>
      Math.abs(a[i] * a[3] - b[i] * b[3]) / 255));
    function append(first, last) {
      const from = colors[first], to = colors[last];
      let farthest = 1, middle = -1;
      for (let index = first + 1; index < last; index++) {
        const progress = (index - first) / (last - first);
        const predicted = from.map((value, channel) => {
          const a = channel < 3 && from[3] === 0 ? to[channel] : value;
          const b = channel < 3 && to[3] === 0 ? from[channel] : to[channel];
          return Math.round(a + (b - a) * progress);
        });
        const deviation = distance(colors[index], predicted);
        if (deviation > farthest) {
          farthest = deviation;
          middle = index;
        }
      }
      if (middle >= 0) {
        append(first, middle);
        append(middle, last);
      } else {
        stops.push({ percent: last, rgba: to });
      }
    }
    stops.push({ percent: 0, rgba: colors[0] });
    append(0, 65);
    append(65, 100);
    return { clear: colors[0], full: colors[100], stops };
  }
  try {
    // The setting has 101 integer positions. Capture pieces that saturate
    // between the default and 100%, as well as layers with a fixed floor.
    for (let percent = 0; percent <= 100; percent++) {
      root.setProperty("--surface-opacity", String(percent / 100), "important");
      for (const { id, css, border, fills, borders } of materials) {
        const tint = css.backgroundImage.match(/^linear-gradient\((rgba?\([^)]*\)), \1\)(?:, none)*$/)?.[1];
        fills.push(color(css.backgroundColor, id === "settingsRangeTrack" ? "" : tint));
        borders.push(color(css["border" + border + "Color"]));
      }
    }
    for (const { id, fills, borders } of materials) {
      const fill = materialRange(fills), border = materialRange(borders);
      if (fill) styles[id].fillRange = fill;
      if (border) styles[id].borderRange = border;
    }
  } finally {
    if (originalOpacity) root.setProperty("--surface-opacity", originalOpacity, opacityPriority);
    else root.removeProperty("--surface-opacity");
    for (const [element, value, priority] of transitions) {
      if (value) element.style.setProperty("transition", value, priority);
      else element.style.removeProperty("transition");
    }
  }
  return styles;
}

export async function readNativeWidgetStyles(page, url, theme, mode) {
  const styles = {};
  const materialOpacity = ["mica", "acrylic", "liquid-glass"].includes(theme);
  for (const [fixture, state, selectors] of samples) {
    url.search = new URLSearchParams({ theme, mode, fixture, state }).toString();
    await page.goto(url.href);
    await page.locator("main[data-fixture]").waitFor();
    await page.evaluate(() => document.fonts.ready);
    await page.mouse.move(450, 390);
    if (fixture === "settings-choices" && state.startsWith("hover-")) {
      await page.getByRole("button", { name: state === "hover-card" ? "System" : "Stable", exact: true }).hover();
    }
    if (fixture === "emoji-favorites" && state === "hover") {
      await page.getByRole("button", { name: "Add emoji" }).hover();
    }
    if (fixture === "backup-dialog") {
      await page.locator(".data-modal-overlay[open]").waitFor();
      if (state === "focus-password") {
        await page.getByRole("radio", { name: "Encrypted backup" }).check();
        await page.getByLabel("Encryption passphrase").focus();
      }
      if (state === "hover-choice") await page.locator(".data-radio-item").last().hover();
      if (state === "hover-selected-choice") await page.locator(".data-radio-item:has(input:checked)").hover();
      if (state === "hover-browse") await page.locator(".btn-icon-export-browse").hover();
      if (state === "hover-cancel") await page.locator(".data-btn-cancel").hover();
      if (state === "hover-submit") await page.locator(".data-btn-primary").hover();
      if (state === "replace-confirmed") await page.getByRole("checkbox", { name: "Confirm replacing clipboard history" }).check();
      if (state === "replace-hover") await page.locator(".data-replace-confirm").hover();
    }
    if (fixture === "settings-data" && state === "hover-path") await page.locator(".data-panel").hover();
    if (fixture === "settings-data" && state === "hover-action") await page.locator(".data-actions-row .btn-icon").first().hover();
    if (fixture === "settings-update-actions" && state === "hover") await page.getByRole("button", { name: "Check now" }).hover();
    if (fixture === "text-fields" && state.startsWith("focus")) {
      await page.getByRole("textbox", { name: state === "focus-text" ? "Entry name" : "Idle timeout" }).focus();
    }
    if (fixture === "context-menu") {
      await page.getByRole("menuitem", { name: "Privacy" }).hover();
    }
    if (fixture === "history-card" && state.endsWith("hover")) {
      await page.locator(".history-item").hover();
      if (state.includes("action") || state.includes("active")) {
        await page.getByRole("button", { name: "Pin", exact: true }).hover();
      }
    }
    if (fixture === "history-ocr" && state === "expanded") {
      await page.getByTestId("ocr-toggle").click();
      await page.getByTestId("ocr-text").waitFor();
      await page.mouse.move(450, 390);
    }
    if (fixture === "scroll-to-top" && state === "hover") {
      await page.getByRole("button", { name: "Scroll to top" }).hover();
    }
    if (fixture === "toast") {
      await page.waitForFunction(() => getComputedStyle(document.querySelector(".toast-item")).opacity === "1");
    }
    await page.addStyleTag({ content: "*, *::before, *::after { transition: none !important; animation: none !important; }" });
    if (fixture === "clipboard-search" && state === "selected") {
      await page.getByRole("button", { name: "图片" }).click();
      await page.mouse.move(450, 390);
    }
    if (fixture === "search-select") {
      const input = page.getByRole("combobox", { name: "Images" });
      await input.click();
      await input.fill("viewer");
      await page.getByRole("option", { name: "Photo Viewer", exact: true }).waitFor();
    }
    if (fixture === "select" && state !== "default" && state !== "disabled") {
      const input = page.getByRole("combobox", { name: "Color mode" });
      if (state === "hover") await input.hover();
      if (state === "focus") await input.focus();
      if (state === "popup") await input.click();
    }
    if (fixture === "privacy-rules" && state === "focus-exclusions") {
      await page.getByRole("textbox", { name: "Excluded applications" }).focus();
    }
    if (fixture === "privacy-detection" && state === "focus") {
      await page.getByRole("textbox", { name: "Patterns" }).focus();
    }
    if (fixture === "disclosure" && state === "expanded") {
      await page.getByRole("button", { name: "Software updates" }).click();
    }
    if (fixture === "help" && state === "expanded") {
      await page.getByRole("button", { name: "Setting help: Software updates" }).click();
    }
    if (fixture === "settings-toggle-help") {
      await page.getByRole("button", { name: "Setting help: Move pasted entries to top" }).click();
    }
    await page.evaluate(async () => {
      await new Promise(resolve => requestAnimationFrame(resolve));
      await Promise.allSettled(document.getAnimations().filter(animation => Number.isFinite(animation.effect.getTiming().iterations))
        .map(animation => animation.finished));
    });
    if (fixture === "settings-slider") {
      if (state === "keyboard") await page.getByRole("slider", { name: "Opacity" }).focus();
      else await page.evaluate(prepareSliderSamples, ".settings-view input[type=range]");
    }
    if (fixture === "preview-toolbar" && state === "default") {
      await page.evaluate(prepareSliderSamples, ".preview-toolbar-opacity input[type=range]");
    }
    Object.assign(styles, await page.evaluate(readStyles, { selectors, materialOpacity }));
  }
  const viewport = page.viewportSize();
  const bounds = await page.locator(".showcase").boundingBox();
  const padding = (await page.evaluate(readStyles, { gallery: [".showcase"] })).gallery.padding;
  styles.gallery = { padding: [
    padding[0] + bounds.y,
    padding[1] + viewport.width - bounds.x - bounds.width,
    padding[2] + Math.max(0, viewport.height - bounds.y - bounds.height),
    padding[3] + bounds.x,
  ] };
  return styles;
}
