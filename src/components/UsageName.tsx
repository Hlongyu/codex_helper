import { Tooltip } from "./ui";

/** Keep the full identity accessible without letting it determine card width. */
export function UsageName({
  name,
  label = name,
}: {
  name: string;
  label?: string;
}) {
  return (
    <Tooltip
      content={
        <span className="usage-name-tooltip">
          {label !== name && <strong>{label}</strong>}
          <span>{name}</span>
        </span>
      }
    >
      <strong className="usage-name" tabIndex={0}>
        {label}
      </strong>
    </Tooltip>
  );
}
