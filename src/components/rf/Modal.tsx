/**
 * Modal — backed by Radix Dialog for a11y.
 * Styled to .rf-modal classes.
 */
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";

export interface ModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  /** Extra classes on the content panel */
  className?: string;
  /** Width override (default 520px via rf-modal in CSS) */
  width?: number | string;
  showCloseButton?: boolean;
}

export function Modal({
  open,
  onOpenChange,
  title,
  children,
  footer,
  className,
  width,
  showCloseButton = true,
}: ModalProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={showCloseButton}
        className={cn(
          "rf-modal p-0 gap-0 border-0 shadow-none bg-transparent overflow-hidden",
          className,
        )}
        style={width ? { maxWidth: typeof width === "number" ? `${width}px` : width } : undefined}
      >
        {title && (
          <DialogHeader className="rf-modal-head">
            <DialogTitle className="text-base" style={{ margin: 0 }}>
              {title}
            </DialogTitle>
          </DialogHeader>
        )}
        {children && (
          <div className="rf-modal-body">
            {children}
          </div>
        )}
        {footer && (
          <DialogFooter className="rf-modal-foot">
            {footer}
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
}
