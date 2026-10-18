import { ident } from "css-tree/utils";
import parse from "css-tree/parser";
import { tokenize, tokenTypes as token } from "css-tree/tokenizer";
import walk from "css-tree/walker";

const BLOCKED_AT_RULES = new Set(["import", "font-face", "namespace", "document", "-moz-document"]);
const BLOCKED_FUNCTIONS = new Set(["url", "src", "image", "image-set", "-webkit-image-set", "expression"]);
const BLOCKED_PROPERTIES = new Set(["behavior", "-moz-binding"]);

/** Inspect the entire token stream, including fragments a CSS AST keeps as Raw. */
export const validateThemeCss = (css: string): void => {
  if (!css.trim()) throw new Error("Theme CSS must not be empty");
  const blocks: number[] = [];
  let hasContent = false;
  tokenize(css, (type, start, end) => {
    if (type !== token.Comment && type !== token.WhiteSpace && type !== token.EOF) hasContent = true;
    const raw = css.slice(start, end);
    if (type === token.Url || type === token.BadUrl || type === token.BadString) throw new Error("Theme CSS cannot load resources or contain malformed strings");
    if (type === token.AtKeyword && BLOCKED_AT_RULES.has(ident.decode(raw.slice(1)).toLowerCase())) throw new Error("Theme CSS contains a forbidden at-rule");
    if (type === token.Function && BLOCKED_FUNCTIONS.has(ident.decode(raw.slice(0, -1)).toLowerCase())) throw new Error("Theme CSS cannot load resources or execute expressions");
    if (type === token.Comment) {
      if (!raw.endsWith("*/")) throw new Error("Theme CSS contains an unterminated comment");
      if (/^\/\*\s*[#@]\s*source(?:mapping)?url\s*=/i.test(raw)) throw new Error("Theme CSS cannot contain debugger resource references");
    }
    if (type === token.String) {
      let escapes = 0;
      for (let index = raw.length - 2; index >= 0 && raw[index] === "\\"; index--) escapes++;
      if (raw.length < 2 || raw[raw.length - 1] !== raw[0] || escapes % 2 !== 0) throw new Error("Theme CSS contains an unterminated string");
    }
    if (type === token.Function || type === token.LeftParenthesis) blocks.push(token.RightParenthesis);
    else if (type === token.LeftCurlyBracket) blocks.push(token.RightCurlyBracket);
    else if (type === token.LeftSquareBracket) blocks.push(token.RightSquareBracket);
    else if (type === token.RightParenthesis || type === token.RightCurlyBracket || type === token.RightSquareBracket) {
      if (blocks.pop() !== type) throw new Error("Theme CSS contains an unmatched block");
    }
    if (blocks.length > 128) throw new Error("Theme CSS nesting exceeds 128 levels");
  });
  if (!hasContent) throw new Error("Theme CSS must contain stylesheet content");
  if (blocks.length !== 0) throw new Error("Theme CSS contains an unterminated block");
  const ast = parse(css, { parseCustomProperty: true, onParseError: () => { /* Raw fragments were checked by the complete token pass. */ } });
  walk(ast, node => {
    if (node.type === "Declaration" && BLOCKED_PROPERTIES.has(ident.decode(node.property).toLowerCase())) {
      throw new Error("Theme CSS contains a forbidden property");
    }
  });
};
