import Dialog, { type ConfirmDialogProps } from "@deziro/ui-react/components/ConfirmDialog";
import { resolveBuiltinTheme } from "../lib/themeRuntime";

type Props = Omit<ConfirmDialogProps, "themeClass"> & { theme: string };

export default function ConfirmDialog({ theme, ...props }: Props) {
  const themeClass = theme.startsWith("custom:") ? undefined : "theme-" + resolveBuiltinTheme(theme);
  return <Dialog {...props} themeClass={themeClass} />;
}
