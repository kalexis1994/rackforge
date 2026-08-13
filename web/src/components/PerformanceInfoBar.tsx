import type { ReactNode } from "react";

export interface PerformanceInfoItem {
  label: string;
  value: string;
}

interface PerformanceInfoBarProps {
  left: PerformanceInfoItem;
  center: PerformanceInfoItem;
  right: PerformanceInfoItem;
  rightAccessory?: ReactNode;
  className?: string;
}

export function PerformanceInfoBar({
  left,
  center,
  right,
  rightAccessory,
  className = "",
}: PerformanceInfoBarProps) {
  return (
    <div className={`performance-info-bar ${className}`.trim()}>
      <div className="performance-info-slot performance-info-left">
        <span>{left.label}</span>
        <strong>{left.value}</strong>
      </div>
      <div className="performance-info-slot performance-info-center">
        <span>{center.label}</span>
        <strong>{center.value}</strong>
      </div>
      <div className="performance-info-slot performance-info-right">
        {rightAccessory}
        <div className="performance-info-copy">
          <span>{right.label}</span>
          <strong>{right.value}</strong>
        </div>
      </div>
    </div>
  );
}
