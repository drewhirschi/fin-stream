import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

export const pendingCheckSurface = {
  bordered: "border-amber-300 bg-amber-50/70",
  row: "bg-amber-50/70",
} as const;

export function isPendingCheck(checkNumber: string | null | undefined) {
  return !checkNumber;
}

export function PendingCheckBadge({
  label = "Pending",
  className,
}: {
  label?: string;
  className?: string;
}) {
  return (
    <Badge
      className={cn("border-amber-300 bg-amber-100 text-amber-950", className)}
    >
      {label}
    </Badge>
  );
}
