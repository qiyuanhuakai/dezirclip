const samples = [
  ["settings-rail", "default", {
    settingsRail: [".settings-rail"], settingsRailItem: [".settings-rail-item:not(.is-active)"],
    settingsRailActive: [".settings-rail-item.is-active"],
  }],
  ["settings-footer", "default", {
    settingsFooterCard: [".settings-footer-card", ".settings-footer-card-label"],
    settingsFooterVersion: [".settings-footer-version"], settingsFooterLink: [".settings-footer-link"],
  }],
  ["history-card", "default", {
    historyCard: [".history-item"], historyMeta: [".item-meta"], historySource: [".item-meta-left .app-info"],
    historyTime: [".item-meta .app-info:last-child span"], historyContent: [".history-item .content-preview"],
    historyAction: [".item-actions .btn-icon"],
  }],
  ["history-card", "selected", { historyCardSelected: [".history-item.selected"] }],
  ["app-header", "default", {
    header: ["header"], headerTop: [".header-top"], headerTitle: [".header-title"],
    headerButton: [".header-top .btn-icon"], headerInput: [".search-input"],
    headerSearch: [".search-container"], headerFilter: [".search-container .hide-scrollbar .btn-icon"],
  }],
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
  }],
  ["search-bar", "loading", { searchSpinner: [".search-bar__spinner"] }],
  ["search-bar", "error", { searchError: [".search-bar__error"] }],
  ["search-bar", "filled", { searchCount: [".search-bar__count"], searchClear: [".search-bar__clear"] }],
  ["highlight", "default", { mark: [".match-highlight mark"], match: [".match-highlight"] }],
  ["search-history", "default", { history: [".search-history"], historyItem: [".search-history__item"] }],
  ["search-history", "empty", { historyEmpty: [".search-history__empty"] }],
  ["disclosure", "default", {
    disclosure: [".settings-disclosure-toggle", ".settings-disclosure-toggle .item-label"],
    disclosureChevron: [".settings-disclosure-chevron"],
  }],
  ["disclosure", "expanded", { disclosureContent: [".settings-disclosure > div:not([hidden])"] }],
  ["help", "default", { help: [".setting-help-button"] }],
  ["help", "expanded", { helpText: [".setting-help-text"], helpActive: [".setting-help-button"] }],
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
    tagInput: [".tag-manager-dialog input"],
  }],
];

function readStyles(selectors) {
  const drawing = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
  const color = value => {
    if (!CSS.supports("color", value)) return [0, 0, 0, 0];
    drawing.clearRect(0, 0, 1, 1);
    drawing.fillStyle = value;
    drawing.fillRect(0, 0, 1, 1);
    return [...drawing.getImageData(0, 0, 1, 1).data];
  };
  const number = value => Math.round((Number.parseFloat(value) || 0) * 100) / 100;
  return Object.fromEntries(Object.entries(selectors).map(([id, [selector, label, pseudo]]) => {
    const element = document.querySelector(selector);
    if (!element) throw Error("Missing fixture element " + selector);
    const style = getComputedStyle(element, pseudo);
    const text = label ? getComputedStyle(document.querySelector(label)) : style;

    const shadows = style.boxShadow.split(/,(?![^()]*\))/).flatMap(layer => {
      const tint = layer.match(/rgba?\([^)]*\)/)?.[0];
      if (!tint) return [];
      const values = layer.replace(tint, "").replace(/\binset\b/, "").trim().split(/\s+/).slice(0, 4).map(number);
      return [{ color: color(tint), values, ...(/\binset\b/.test(layer) ? { inset: true } : {}) }];
    }).reverse();
    const sides = ["Top", "Right", "Bottom", "Left"];
    const borderWidths = sides.map(side => number(style["border" + side + "Width"]));
    const borderSide = sides[borderWidths.findIndex(width => width > 0)] ?? "Top";
    const outlineWidth = style.outlineStyle === "solid" ? number(style.outlineWidth) : 0;
    const gradient = style.backgroundImage.match(/^linear-gradient\(([\d.]+)deg, (rgba?\([^)]*\)) 0%, (rgba?\([^)]*\)) 100%\)$/);
    const letterSpacing = number(text.letterSpacing);
    return [id, {
      fill: color(style.backgroundColor), text: color(text.color), border: color(style["border" + borderSide + "Color"]),
      borderWidth: borderWidths[0], radius: number(style.borderTopLeftRadius),
      ...(style["border" + borderSide + "Style"] === "dashed" ? { dashed: true } : {}),
      ...(borderWidths.some(width => width !== borderWidths[0]) ? { borderWidths } : {}),
      ...(outlineWidth > 0 && number(style.outlineOffset) === -outlineWidth
        ? { outline: { width: outlineWidth, color: color(style.outlineColor) } } : {}),
      ...(gradient ? { gradient: { angle: Number(gradient[1]), from: color(gradient[2]), to: color(gradient[3]) } } : {}),
      ...(letterSpacing ? { letterSpacing } : {}),
      ...(id === "historyCard" || id === "historyCardSelected"
        ? { margin: sides.map(side => number(style["margin" + side])) } : {}),
      width: element.offsetWidth, height: element.offsetHeight, marginTop: number(style.marginTop),
      padding: [style.paddingTop, style.paddingRight, style.paddingBottom, style.paddingLeft].map(number),
      font: text.fontFamily.split(",")[0].trim().replace(/^['"]|['"]$/g, ""),
      size: number(text.fontSize), weight: number(text.fontWeight), uppercase: text.textTransform === "uppercase",
      opacity: number(style.opacity),
      ...(shadows.length ? { shadows } : {}),
    }];
  }));
}

export async function readNativeWidgetStyles(page, url, theme, mode) {
  const styles = {};
  for (const [fixture, state, selectors] of samples) {
    url.search = new URLSearchParams({ theme, mode, fixture, state }).toString();
    await page.goto(url.href);
    await page.locator("main[data-fixture]").waitFor();
    await page.evaluate(() => document.fonts.ready);
    await page.mouse.move(450, 390);
    if (fixture === "toast") {
      await page.waitForFunction(() => getComputedStyle(document.querySelector(".toast-item")).opacity === "1");
    }
    await page.addStyleTag({ content: "*, *::before, *::after { transition: none !important; animation: none !important; }" });
    if (fixture === "select" && state !== "default" && state !== "disabled") {
      const input = page.getByRole("combobox", { name: "Color mode" });
      if (state === "hover") await input.hover();
      if (state === "focus") await input.focus();
      if (state === "popup") await input.click();
    }
    if (fixture === "disclosure" && state === "expanded") {
      await page.getByRole("button", { name: "Software updates" }).click();
    }
    if (fixture === "help" && state === "expanded") {
      await page.getByRole("button", { name: "Setting help: Software updates" }).click();
    }
    await page.evaluate(async () => {
      await new Promise(resolve => requestAnimationFrame(resolve));
      await Promise.allSettled(document.getAnimations().filter(animation => Number.isFinite(animation.effect.getTiming().iterations))
        .map(animation => animation.finished));
    });
    Object.assign(styles, await page.evaluate(readStyles, selectors));
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
