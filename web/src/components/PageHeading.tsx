export function PageHeading({
  eyebrow,
  title,
  detail,
}: {
  /** Left out where the title already says it all. */
  eyebrow?: string;
  title: string;
  detail: string;
}) {
  return (
    <div className="page-heading">
      {eyebrow ? <span className="eyebrow accent">{eyebrow}</span> : null}
      <h1>{title}</h1>
      <p>{detail}</p>
    </div>
  );
}
