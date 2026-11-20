import { ArrowUp } from "lucide-react";

export function ScrollToTopButton({ label, onClick }: { label: string; onClick: () => void }) {
  return <button type="button" className="btn-icon scroll-top-button" onClick={onClick} aria-label={label} title={label}>
    <ArrowUp size={16} />
  </button>;
}
