import { useState } from "react";
import type { ReactNode } from "react";
import { present } from "../lib/inspector";
import styles from "./Inspector.module.scss";

export function PropertyRows({ rows }: { rows: [string, ReactNode][] }) {
	const available = rows.filter(([, value]) => value != null && value !== "");
	if (available.length === 0) return null;
	return (
		<dl>
			{available.map(([label, value]) => (
				<div key={label}>
					<dt>{label}</dt>
					<dd>{value}</dd>
				</div>
			))}
		</dl>
	);
}

export function CopyValue({ value, label }: { value: string; label: string }) {
	const [status, setStatus] = useState("");
	const long = value.length > 120;
	return (
		<div className={styles.copyValue}>
			<span
				className={long ? styles.clippedValue : undefined}
				title={long ? value : undefined}
			>
				{value}
			</span>
			{typeof navigator.clipboard?.writeText === "function" && (
				<button
					type="button"
					aria-label={`Copy ${label}`}
					title={`Copy ${label}`}
					onClick={async () => {
						try {
							await navigator.clipboard.writeText(value);
							setStatus("Copied");
						} catch {
							setStatus("Could not copy. Select the text to copy it manually.");
						}
					}}
				>
					<svg viewBox="0 0 18 18" aria-hidden="true">
						<path d="M6 6h9v9H6zM3 12V3h9" />
					</svg>
				</button>
			)}
			{status && (
				<span role="status" className={styles.copyStatus}>
					{status}
				</span>
			)}
			{long && (
				<details className={styles.fullValue}>
					<summary>Show full {label}</summary>
					<span>{value}</span>
				</details>
			)}
		</div>
	);
}

// Round presentation only; Advanced preserves exact keys and values.
export function MetadataValues({ value, exact = false }: { value: unknown; exact?: boolean }) {
	if (value == null || !present(value)) return null;
	if (Array.isArray(value)) {
		if (value.every((item) => typeof item === "string")) return <span>{value.join(", ")}</span>;
		return (
			<div className={styles.metadataItems}>
				{value.filter(present).map((item, index) => (
					<details key={index}>
						<summary>Item {index + 1}</summary>
						<MetadataValues value={item} exact={exact} />
					</details>
				))}
			</div>
		);
	}
	if (typeof value === "object")
		return (
			<PropertyRows
				rows={Object.entries(value)
					.filter(([, item]) => present(item))
					.map(([key, item]) => [
						exact
							? key
							: key
									.replace(/([a-z])([A-Z])/g, "$1 $2")
									.replace(/^./, (letter) => letter.toUpperCase()),
						<MetadataValues value={item} exact={exact} />,
					])}
			/>
		);
	return (
		<span className={styles.multiline}>
			{typeof value === "boolean"
				? value
					? "Yes"
					: "No"
				: typeof value === "number" && !exact
					? value.toLocaleString(undefined, { maximumFractionDigits: 3 })
					: String(value)}
		</span>
	);
}
