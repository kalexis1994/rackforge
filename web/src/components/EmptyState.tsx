import { BrandMark } from "../components/BrandMark";

export function EmptyState({ title }: { title: string }) {
  return (
    <div className="empty-state">
      {/* Was the letters "RF" on an accent square — a stand-in from before the
          mark existed. There is a real one now, and it follows the lighting. */}
      <BrandMark />
      <h2>{title}</h2>
      <p>RackForge will update this view as soon as Core is available.</p>
    </div>
  );
}
