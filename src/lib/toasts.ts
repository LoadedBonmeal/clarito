import { toast } from "sonner";

export const notify = {
  success: (msg: string) => toast.success(msg),
  error:   (msg: string) => toast.error(msg),
  info:    (msg: string) => toast.info(msg),
  warn:    (msg: string) => toast.warning(msg),
  promise: <T>(
    p: Promise<T>,
    msgs: { loading: string; success: string; error: string },
  ) => toast.promise(p, msgs),
};
