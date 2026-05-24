type Translate = (key: string) => string;

export const getClipboardTypeName = (type: string, t: Translate): string => {
  switch (type) {
    case "code":
      return t("type_code");
    case "link":
    case "url":
      return t("type_url");
    case "file":
      return t("type_file");
    case "image":
      return t("type_image");
    case "video":
      return t("type_video");
    case "rich_text":
      return t("type_rich_text");
    default:
      return t("type_text") || "Text";
  }
};
