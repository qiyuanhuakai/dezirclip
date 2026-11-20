import type { ButtonHTMLAttributes, ComponentPropsWithoutRef, ReactNode, Ref } from "react";
import { motion, type HTMLMotionProps } from "framer-motion";
import { ImageOff } from "lucide-react";

export interface HistoryCardProps extends HTMLMotionProps<"div"> {
  selected?: boolean;
  compact?: boolean;
  pinned?: boolean;
  disableLayout?: boolean;
  ref?: Ref<HTMLDivElement>;
}

export function HistoryCard({ selected, compact, pinned, disableLayout, className = "", ...props }: HistoryCardProps) {
  const classes = ["history-item", selected && "selected", compact && "compact", pinned && "pinned", className]
    .filter(Boolean).join(" ");
  return <motion.div {...props} data-test-clipboard-item className={classes}
    layout={!disableLayout} initial={false} animate={disableLayout ? undefined : { marginBottom: 0 }}
    exit={disableLayout ? undefined : { opacity: 0, scale: 0.95 }} transition={disableLayout ? undefined : { duration: 0.1 }} />;
}

export function HistoryCardContent({ rich, concealed, className = "", children, ...props }: ComponentPropsWithoutRef<"div"> & {
  rich?: boolean;
  concealed?: boolean;
}) {
  const classes = ["content-preview", rich && "rich-text", concealed && "privacy-concealed", className]
    .filter(Boolean).join(" ");
  return <div {...props} className={classes}>
    {concealed ? <div className="privacy-hidden-content">{children}</div> : children}
  </div>;
}

export function HistoryImagePreview({ source, missing, pending, unavailable, alt, ...props }: {
  source?: string | null; missing?: string; pending?: boolean; unavailable?: string; alt: string;
} & Pick<ComponentPropsWithoutRef<"img">, "draggable" | "onError">) {
  if (missing) return <div className="image-preview error-placeholder history-image-missing" role="status">
    <ImageOff size={24} /><span>{missing}</span>
  </div>;
  if (pending) return <div className="image-preview image-preview-pending" aria-hidden="true" />;
  if (source) return <img className="image-preview" src={source} alt={alt} loading="lazy" {...props} />;
  return <div className="image-preview image-preview-unavailable" role="status">
    <ImageOff size={20} /><span>{unavailable}</span>
  </div>;
}

export function HistoryCardActions({ children }: { children?: ReactNode }) {
  return <div className="item-actions">{children}</div>;
}

export interface HistoryCardActionProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  active?: boolean;
}

export function HistoryCardAction({ active, className = "", ...props }: HistoryCardActionProps) {
  return <button type="button" aria-pressed={active}
    className={["btn-icon", active && "active", className].filter(Boolean).join(" ")} {...props} />;
}

export interface HistoryCardMetaProps {
  source: string;
  sourceIcon?: ReactNode;
  leading?: ReactNode;
  actions?: ReactNode;
  status?: ReactNode;
  time?: string;
}

export function HistoryCardMeta({ source, sourceIcon, leading, actions, status, time }: HistoryCardMetaProps) {
  return <div className="item-meta">
    <div className="item-meta-left">
      {leading}
      <div className="app-info">{sourceIcon}<span>{source}</span></div>
    </div>
    <div className="history-card-meta-right" style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <HistoryCardActions>{actions}</HistoryCardActions>
      <div className="app-info" style={{ opacity: 0.6, fontSize: "10px", display: "flex", gap: "6px", alignItems: "center" }}>
        {status}<span>{time}</span>
      </div>
    </div>
  </div>;
}
