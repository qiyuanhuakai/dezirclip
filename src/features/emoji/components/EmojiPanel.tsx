import { useEffect, useMemo, useState } from "react";
import type { DragEvent } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { Plus, X } from "lucide-react";

interface EmojiPanelProps {
  t: (key: string) => string;
  favorites: string[];
  setFavorites: (val: string[] | ((prev: string[]) => string[])) => void;
  activeTab: "emoji" | "favorites";
  setActiveTab: (val: "emoji" | "favorites") => void;
  saveSetting: (key: string, val: string) => void;
}

type EmojiGroup = { name: string; emojis: string[] };
type EmojiData = { groups?: EmojiGroup[] };

const FALLBACK_GROUPS: EmojiGroup[] = [
  {
    name: "常用",
    emojis: ["😀", "😁", "😂", "🤣", "😊", "😍", "😘", "😎", "🤔", "😅", "😭", "😡", "👍", "👎", "🙏", "👏", "🎉", "🔥", "💯", "✨", "👌", "😴", "🥳", "🤩", "😬", "😇", "🤝", "🙌"]
  },
  {
    name: "表情",
    emojis: ["🙂", "😇", "🙃", "😉", "😌", "🤗", "🤩", "🥳", "😴", "😪", "😤", "😱", "🤯", "😵", "🤐", "🫠", "🫡", "🫣", "😐", "😑", "😶", "🙄", "😮", "😯", "😲", "🥺", "😢", "😥", "😓", "😕"]
  },
  {
    name: "手势",
    emojis: ["👌", "✌️", "🤞", "🤟", "🤘", "🤙", "👊", "✊", "🤚", "🖐️", "✋", "👋", "🫶", "👉", "👈", "👇", "👆", "🫵", "🤝", "🙌", "🤲", "🤜", "🤛", "🫰", "🤌"]
  },
  {
    name: "人物",
    emojis: ["👨‍💻", "👩‍💻", "👨‍🎨", "👩‍🎨", "👨‍🚀", "👩‍🚀", "👨‍🍳", "👩‍🍳", "👨‍⚕️", "👩‍⚕️", "👨‍🏫", "👩‍🏫", "🧑‍💼", "🧑‍🔧", "🧑‍🎧", "🧑‍🚒"]
  },
  {
    name: "动物",
    emojis: ["🐶", "🐱", "🐭", "🐹", "🐰", "🦊", "🐻", "🐼", "🐯", "🦁", "🐮", "🐷", "🐸", "🐵", "🐔", "🐧", "🐦", "🐤", "🐺", "🦄"]
  },
  {
    name: "美食",
    emojis: ["🍎", "🍐", "🍊", "🍋", "🍉", "🍇", "🍓", "🍒", "🍍", "🥭", "🍔", "🍟", "🍕", "🌭", "🍣", "🍤", "🍜", "🍲", "🍰", "🍩"]
  },
  {
    name: "活动",
    emojis: ["⚽", "🏀", "🏈", "⚾", "🎾", "🏐", "🏓", "🏸", "🥊", "🏆", "🎯", "🎮", "🎲", "🎹", "🎸", "🎤", "🎧", "🏃", "🚴", "🧘"]
  },
  {
    name: "旅行",
    emojis: ["🚗", "🚕", "🚌", "🚎", "🏎️", "🚓", "🚑", "🚒", "🚀", "✈️", "🛫", "🛬", "🚢", "⛵", "🗺️", "🧭", "🏝️", "⛰️", "🌋", "🏜️"]
  },
  {
    name: "物品",
    emojis: ["📱", "💻", "🖥️", "⌨️", "🖱️", "📷", "🎥", "📺", "🔦", "💡", "🔋", "🔌", "📦", "📌", "✏️", "📚", "🧰", "🧲", "🧯", "🧪"]
  },
  {
    name: "符号",
    emojis: ["❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "🤍", "🤎", "💔", "❗", "❓", "✅", "❌", "⚠️", "⭕", "💯", "✨", "⭐", "🌟"]
  }
];

const IMAGE_EXTS = new Set(["png", "jpg", "jpeg", "webp", "gif"]);

const normalizePath = (path: string) => path.trim();

const isImagePath = (path: string) => {
  const ext = path.split(".").pop()?.toLowerCase() || "";
  return IMAGE_EXTS.has(ext);
};

const isImageFile = (file: File) => {
  if (file.type && file.type.startsWith("image/")) return true;
  const ext = file.name.split(".").pop()?.toLowerCase() || "";
  return IMAGE_EXTS.has(ext);
};

const blobToDataUrl = (blob: Blob) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") resolve(reader.result);
      else reject(new Error("Invalid file result"));
    };
    reader.onerror = () => reject(reader.error || new Error("File read failed"));
    reader.readAsDataURL(blob);
  });

const fileToDataUrl = (file: File) => blobToDataUrl(file);

const parseSrcset = (srcset: string) => {
  const first = srcset.split(",")[0]?.trim() || "";
  if (!first) return "";
  return first.split(/\s+/)[0] || "";
};

const collectImageUrlsFromHtml = (html: string) => {
  try {
    const doc = new DOMParser().parseFromString(html, "text/html");
    const urls: string[] = [];
    doc.querySelectorAll("img").forEach((img) => {
      const src = img.getAttribute("src") || "";
      const srcset = img.getAttribute("srcset") || "";
      if (src) urls.push(src);
      const srcsetUrl = parseSrcset(srcset);
      if (srcsetUrl) urls.push(srcsetUrl);
    });
    doc.querySelectorAll("source").forEach((source) => {
      const src = source.getAttribute("src") || "";
      const srcset = source.getAttribute("srcset") || "";
      if (src) urls.push(src);
      const srcsetUrl = parseSrcset(srcset);
      if (srcsetUrl) urls.push(srcsetUrl);
    });
    doc.querySelectorAll("a[href]").forEach((anchor) => {
      const href = anchor.getAttribute("href") || "";
      if (href) urls.push(href);
    });
    return urls;
  } catch {
    return [];
  }
};

const getDropUrls = (dt: DataTransfer | null) => {
  if (!dt) return [];
  const urls: string[] = [];
  const uriList = dt.getData("text/uri-list");
  if (uriList) {
    uriList
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line && !line.startsWith("#"))
      .forEach((line) => urls.push(line));
  }
  const html = dt.getData("text/html");
  if (html) {
    urls.push(...collectImageUrlsFromHtml(html));
  }
  const plain = dt.getData("text/plain");
  if (plain) {
    urls.push(plain.trim());
  }
  return Array.from(
    new Set(
      urls
        .map((u) => u.trim())
        .filter((u) => u.length > 0)
    )
  );
};

const resolveDropPaths = (payload: unknown): string[] => {
  if (Array.isArray(payload)) {
    return payload.filter((p): p is string => typeof p === "string");
  }
  if (payload && typeof payload === "object" && "paths" in payload) {
    const maybePaths = (payload as { paths?: unknown }).paths;
    if (Array.isArray(maybePaths)) {
      return maybePaths.filter((p): p is string => typeof p === "string");
    }
  }
  return [];
};

const dedupeFavoritePaths = (paths: string[]) =>
  Array.from(
    new Set(
      paths
        .map(normalizePath)
        .filter((path) => path && isImagePath(path))
    )
  );

const EmojiPanel = ({ t, favorites, setFavorites, activeTab, setActiveTab, saveSetting }: EmojiPanelProps) => {
  const [isDragging, setIsDragging] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [emojiGroups, setEmojiGroups] = useState<EmojiGroup[]>(FALLBACK_GROUPS);

  const flatEmoji = useMemo(() => emojiGroups.flatMap((g) => g.emojis), [emojiGroups]);
  const hasFavorites = favorites.length > 0;

  const persistFavorites = (updater: string[] | ((prev: string[]) => string[])) => {
    setFavorites((prev) => {
      const next = dedupeFavoritePaths(typeof updater === "function" ? updater(prev) : updater);
      saveSetting("app.emoji_favorites", JSON.stringify(next));
      return next;
    });
  };

  const removeFavoritePath = (path: string) => {
    persistFavorites((prev) => prev.filter((p) => p !== path));
    invoke("remove_emoji_favorite", { path }).catch(console.error);
  };

  useEffect(() => {
    if (activeTab !== "favorites") return;

    let cancelled = false;
    invoke<string[]>("list_emoji_favorites")
      .then((diskPaths) => {
        if (cancelled) return;
        const merged = dedupeFavoritePaths([...favorites, ...(Array.isArray(diskPaths) ? diskPaths : [])]);
        const current = dedupeFavoritePaths(favorites);
        if (
          merged.length === current.length &&
          merged.every((path, index) => path === current[index])
        ) {
          return;
        }
        persistFavorites(merged);
      })
      .catch(console.error);

    return () => {
      cancelled = true;
    };
  }, [activeTab, favorites]);

  const addFavoritePaths = async (paths: string[]) => {
    const normalized = paths.map(normalizePath).filter((p) => p && isImagePath(p));
    if (normalized.length === 0) return;
    const saved = await Promise.all(
      normalized.map(async (path) => {
        try {
          return await invoke<string>("save_emoji_favorite", { sourcePath: path });
        } catch (e) {
          console.warn("Failed to save emoji favorite:", e);
          return null;
        }
      })
    );
    const valid = saved.filter((p): p is string => typeof p === "string" && p.length > 0);
    if (valid.length === 0) return;
    persistFavorites((prev) => Array.from(new Set([...prev, ...valid])));
  };

  const addFavoriteFiles = async (files: FileList | File[]) => {
    const fileList = files instanceof FileList ? Array.from(files) : files;
    const paths: string[] = [];
    const dataUrlFiles: { dataUrl: string; fileName: string }[] = [];

    for (const file of fileList) {
      if (!isImageFile(file)) continue;
      const filePath = (file as { path?: string }).path;
      if (filePath) {
        paths.push(filePath);
      } else {
        try {
          const dataUrl = await fileToDataUrl(file);
          dataUrlFiles.push({ dataUrl, fileName: file.name });
        } catch (e) {
          console.warn("Failed to read dropped file:", e);
        }
      }
    }

    if (paths.length > 0) {
      await addFavoritePaths(paths);
    }

    if (dataUrlFiles.length > 0) {
      const saved = await Promise.all(
        dataUrlFiles.map(async ({ dataUrl, fileName }) => {
          try {
            return await invoke<string>("save_emoji_favorite_data_url", { dataUrl, fileName });
          } catch (e) {
            console.warn("Failed to save dropped data url:", e);
            return null;
          }
        })
      );
      const valid = saved.filter((p): p is string => typeof p === "string" && p.length > 0);
      if (valid.length > 0) {
        persistFavorites((prev) => Array.from(new Set([...prev, ...valid])));
      }
    }
  };

  const addFavoriteDataUrls = async (dataUrls: string[]) => {
    const normalized = dataUrls.map((url) => url.trim()).filter((url) => url.startsWith("data:"));
    if (normalized.length === 0) return;
    const saved = await Promise.all(
      normalized.map(async (dataUrl) => {
        try {
          return await invoke<string>("save_emoji_favorite_data_url", { dataUrl });
        } catch (e) {
          console.warn("Failed to save dropped data url:", e);
          return null;
        }
      })
    );
    const valid = saved.filter((p): p is string => typeof p === "string" && p.length > 0);
    if (valid.length > 0) {
      persistFavorites((prev) => Array.from(new Set([...prev, ...valid])));
    }
  };

  const handleSend = async (content: string, contentType: string) => {
    await invoke("copy_to_clipboard", {
      content,
      contentType,
      paste: true,
      id: 0,
      deleteAfterUse: false,
      pasteWithFormat: false
    });
  };

  const handleTabChange = (tab: "emoji" | "favorites") => {
    setActiveTab(tab);
    saveSetting("app.emoji_panel_tab", tab);
  };

  const handleSelectFiles = async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }]
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    void addFavoritePaths(paths);
  };

  const getFilesFromDataTransfer = (dt: DataTransfer | null): File[] => {
    if (!dt) return [];
    if (dt.files && dt.files.length > 0) {
      return Array.from(dt.files);
    }
    const files: File[] = [];
    if (dt.items) {
      for (let i = 0; i < dt.items.length; i++) {
        const item = dt.items[i];
        if (item.kind === "file") {
          const file = item.getAsFile();
          if (file) files.push(file);
        }
      }
    }
    return files;
  };

  const handleDomFiles = async (files: File[] | FileList | null | undefined) => {
    if (!files) return;
    const fileList = files instanceof FileList ? Array.from(files) : files;
    if (fileList.length === 0) return;
    await addFavoriteFiles(fileList);
  };

  const urlToDataUrl = async (url: string): Promise<string | null> => {
    try {
      const response = await fetch(url);
      if (!response.ok) return null;
      const blob = await response.blob();
      return blobToDataUrl(blob);
    } catch {
      return null;
    }
  };

  const handleDomDropDataTransfer = async (dt: DataTransfer | null) => {
    const files = getFilesFromDataTransfer(dt);
    if (files.length > 0) {
      await handleDomFiles(files);
      return;
    }
    const urls = getDropUrls(dt);
    if (urls.length === 0) return;
    const dataUrls = urls.filter((url) => url.startsWith("data:"));
    if (dataUrls.length > 0) {
      await addFavoriteDataUrls(dataUrls);
    }
    const httpUrls = urls.filter((url) => url.startsWith("http://") || url.startsWith("https://"));
    if (httpUrls.length > 0) {
      const fetchedDataUrls = (await Promise.all(httpUrls.map(urlToDataUrl))).filter(
        (u): u is string => u !== null
      );
      if (fetchedDataUrls.length > 0) {
        await addFavoriteDataUrls(fetchedDataUrls);
      }
    }
  };

  const handleDomDrop = async (event: DragEvent<HTMLDivElement>) => {
    await handleDomDropDataTransfer(event.dataTransfer);
  };

  useEffect(() => {
    let alive = true;
    fetch("/emoji-data.json")
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error("Failed to load emoji data"))))
      .then((data: EmojiData) => {
        if (!alive) return;
        const groups = Array.isArray(data?.groups) ? data.groups.filter((g) => g && Array.isArray(g.emojis)) : [];
        if (groups.length > 0) {
          setEmojiGroups(groups);
        }
      })
      .catch(() => {
        if (alive) setEmojiGroups(FALLBACK_GROUPS);
      });

    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    const appWindow = getCurrentWindow();

    const unlistenDrop = appWindow.listen("tauri://file-drop", (e) => {
      const paths = resolveDropPaths(e.payload);
      if (paths.length > 0) void addFavoritePaths(paths);
      setIsDragging(false);
    });
    const unlistenHover = appWindow.listen("tauri://file-drop-hover", () => {
      setIsDragging(true);
    });
    const unlistenCancel = appWindow.listen("tauri://file-drop-cancelled", () => {
      setIsDragging(false);
    });
    const unlistenV2Drop = appWindow.listen("tauri://drag-drop", (e) => {
      const paths = resolveDropPaths(e.payload);
      if (paths.length > 0) void addFavoritePaths(paths);
      setIsDragging(false);
    });
    const unlistenV2Enter = appWindow.listen("tauri://drag-enter", () => {
      setIsDragging(true);
    });
    const unlistenV2Leave = appWindow.listen("tauri://drag-leave", () => {
      setIsDragging(false);
    });
    const unlistenNativeEmoji = appWindow.listen("emoji-favorite-drop", (e) => {
      const payload = e.payload as unknown;
      const paths = resolveDropPaths(payload);
      if (paths.length === 0) return;
      const alreadySaved =
        typeof payload === "object" &&
        payload !== null &&
        "alreadySaved" in payload &&
        Boolean((payload as { alreadySaved?: boolean }).alreadySaved);
      if (alreadySaved) {
        persistFavorites((prev) => Array.from(new Set([...prev, ...paths])));
      } else {
        void addFavoritePaths(paths);
      }
      setIsDragging(false);
    });

    return () => {
      unlistenDrop.then((f) => f());
      unlistenHover.then((f) => f());
      unlistenCancel.then((f) => f());
      unlistenV2Drop.then((f) => f());
      unlistenV2Enter.then((f) => f());
      unlistenV2Leave.then((f) => f());
      unlistenNativeEmoji.then((f) => f());
    };
  }, [favorites]);

  useEffect(() => {
    const handleDragOver = (event: globalThis.DragEvent) => {
      event.preventDefault();
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = "copy";
      }
      if (!isDragging) setIsDragging(true);
    };

    const handleDragLeave = (event: globalThis.DragEvent) => {
      if (event.relatedTarget === null) {
        setIsDragging(false);
      }
    };

    const handleDrop = (event: globalThis.DragEvent) => {
      event.preventDefault();
      setIsDragging(false);
      void handleDomDropDataTransfer(event.dataTransfer);
    };

    window.addEventListener("dragover", handleDragOver);
    window.addEventListener("dragleave", handleDragLeave);
    window.addEventListener("drop", handleDrop);

    return () => {
      window.removeEventListener("dragover", handleDragOver);
      window.removeEventListener("dragleave", handleDragLeave);
      window.removeEventListener("drop", handleDrop);
    };
  }, [isDragging]);

  return (
    <div className="emoji-panel">
      <div className="emoji-tabs">
        <button
          className={`btn-icon emoji-tab ${activeTab === "emoji" ? "active" : ""}`}
          onClick={() => handleTabChange("emoji")}
        >
          {t("emoji_tab") || "Emoji"}
        </button>
        <button
          className={`btn-icon emoji-tab ${activeTab === "favorites" ? "active" : ""}`}
          onClick={() => handleTabChange("favorites")}
        >
          {t("emoji_favorites") || "收藏"}
        </button>
      </div>

      {activeTab === "emoji" && (
        <div className="emoji-content">
          {emojiGroups.map((group) => (
            <div key={group.name} className="emoji-group">
              <div className="emoji-group-title">{group.name}</div>
              <div className="emoji-grid">
                {group.emojis.map((emoji) => (
                  <button
                    key={`${group.name}-${emoji}`}
                    className="emoji-btn"
                    onClick={() => handleSend(emoji, "text")}
                    title={emoji}
                  >
                    {emoji}
                  </button>
                ))}
              </div>
            </div>
          ))}
          {flatEmoji.length === 0 && (
            <div className="emoji-empty">{t("emoji_empty") || "暂无表情"}</div>
          )}
        </div>
      )}

      {activeTab === "favorites" && (
        <div
          className={`emoji-content ${isDragging ? "dragging" : ""}`}
          onClick={() => setDeleteTarget(null)}
          onContextMenu={(e) => {
            if ((e.target as HTMLElement).closest(".emoji-fav-card")) return;
            setDeleteTarget(null);
          }}
          onDragOver={(e) => {
            e.preventDefault();
            if (!isDragging) setIsDragging(true);
          }}
          onDragLeave={(e) => {
            if (e.currentTarget.contains(e.relatedTarget as Node)) return;
            setIsDragging(false);
          }}
          onDrop={(e) => {
            e.preventDefault();
            setIsDragging(false);
            void handleDomDrop(e);
          }}
        >
          <div className={`emoji-fav-grid ${isDragging ? "dragging" : ""}`}>
            <div className="emoji-fav-card emoji-fav-add">
              <button
                className="emoji-fav-preview emoji-fav-add-btn"
                onClick={(e) => {
                  e.stopPropagation();
                  void handleSelectFiles();
                }}
                title={t("emoji_add_files") || "添加表情"}
              >
                <Plus size={18} />
                <span>{t("emoji_add_files") || "添加表情"}</span>
              </button>
            </div>
            {favorites.map((path) => {
              const name = path.split(/[/\\]/).pop() || path;
              const isDeleteVisible = deleteTarget === path;
              return (
                <div
                  key={path}
                  className="emoji-fav-card"
                  data-delete-visible={isDeleteVisible}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    setDeleteTarget(path);
                  }}
                >
                  <button
                    className="btn-icon emoji-fav-remove"
                    title={t("delete") || "删除"}
                    onClick={(e) => {
                      e.stopPropagation();
                      removeFavoritePath(path);
                    }}
                  >
                    <X size={12} />
                  </button>
                  <button
                    className="emoji-fav-preview"
                    title={name}
                    onClick={() => handleSend(path, "image")}
                  >
                    <img
                      src={convertFileSrc(path)}
                      alt={name}
                      onError={(e) => {
                        e.currentTarget.onerror = null;
                        removeFavoritePath(path);
                      }}
                    />
                  </button>
                </div>
              );
            })}
          </div>

          {!hasFavorites && (
            <div className="emoji-fav-empty">
              <div className="emoji-empty-title">{t("emoji_empty") || "暂无表情"}</div>
              <div className="emoji-empty-desc">{t("emoji_fav_hint") || "点击或拖拽图片快速添加"}</div>
            </div>
          )}
          {hasFavorites && (
            <div className="emoji-fav-tip">{t("emoji_fav_tip") || "可直接拖拽表情添加"}</div>
          )}
        </div>
      )}
    </div>
  );
};

export default EmojiPanel;
