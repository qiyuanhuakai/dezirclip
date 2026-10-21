const PROTECTED_CONTENT = "[data-privacy-protected]";

/**
 * Native browser copy/cut cannot attach Core's protected clipboard authorization.
 * Explicit application copy actions use the authorized backend instead.
 */
export function installPrivacyNativeCopyGuard(owner: Document = document): () => void {
  const guard = (event: Event) => {
    const block = () => {
      event.preventDefault();
      event.stopImmediatePropagation();
    };

    let protectedNodes: NodeListOf<Element>;
    try {
      protectedNodes = owner.querySelectorAll(PROTECTED_CONTENT);
    } catch {
      block();
      return;
    }
    if (protectedNodes.length === 0) return;

    try {
      const target = event.target;
      if (!(target instanceof Node)) {
        block();
        return;
      }
      const element = target instanceof Element ? target : target.parentElement;
      if (element?.closest(PROTECTED_CONTENT)) {
        block();
        return;
      }

      const selection = owner.getSelection();
      if (!selection || !Number.isSafeInteger(selection.rangeCount) || selection.rangeCount < 0) {
        block();
        return;
      }
      for (let index = 0; index < selection.rangeCount; index += 1) {
        const range = selection.getRangeAt(index);
        const ancestor = range.commonAncestorContainer;
        if (!ancestor.isConnected || (ancestor !== owner && ancestor.ownerDocument !== owner)) {
          block();
          return;
        }
        for (const node of protectedNodes) {
          if (range.intersectsNode(node)) {
            block();
            return;
          }
        }
      }
    } catch {
      // An uncertain selection must not fall through to a native plaintext write.
      block();
    }
  };

  owner.addEventListener("copy", guard, true);
  owner.addEventListener("cut", guard, true);
  return () => {
    owner.removeEventListener("copy", guard, true);
    owner.removeEventListener("cut", guard, true);
  };
}
