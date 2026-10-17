import type { ImportedMetadata } from "../services/medal";
import { time } from "../lib/format";
import styles from "./Inspector.module.scss";

function date(value: number | null) {
	return value == null
		? null
		: new Date(value).toLocaleString() + " · " + new Date(value).toISOString();
}
function label(key: string) {
	return key.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/^./, (value) => value.toUpperCase());
}
function Properties({ value }: { value: unknown }) {
	if (value == null) return null;
	if (typeof value !== "object")
		return <span>{typeof value === "boolean" ? (value ? "Yes" : "No") : String(value)}</span>;
	if (Array.isArray(value))
		return (
			<>
				{value.map((item, index) => (
					<details key={index}>
						<summary>Item {index + 1}</summary>
						<Properties value={item} />
					</details>
				))}
			</>
		);
	return (
		<dl>
			{Object.entries(value).map(([key, item]) =>
				item == null ? null : (
					<div key={key}>
						<dt>{label(key)}</dt>
						<dd>
							<Properties value={item} />
						</dd>
					</div>
				),
			)}
		</dl>
	);
}
export default function MedalDetails({ entries }: { entries: ImportedMetadata[] }) {
	return (
		<section aria-label="Imported Medal metadata">
			<h2>Medal metadata</h2>
			{entries.map((entry, index) => {
				const data = entry.data;
				const fields: [string, string | null][] = [
					["Title", data.title],
					["Title source", data.titleField],
					["Description", data.description],
					["Game", data.game],
					["Game ID", data.gameId],
					["Medal created", date(data.createdMs)],
					["Content created", date(data.contentCreatedMs)],
					["Last edit", date(data.lastEditMs)],
					["Provider duration", data.duration == null ? null : time(data.duration)],
					["Tags", data.tags.length ? data.tags.join(", ") : null],
				];
				return (
					<details
						className={styles.technical}
						key={entry.externalId + ":" + entry.relation + ":" + index}
						open={index === 0}
					>
						<summary>
							{entry.relation === "original"
								? "Related Medal edit (this file is its original)"
								: "Imported clip information"}
						</summary>
						{entry.relation === "original" && (
							<p>
								These fields describe a related edit. They do not replace this
								recording’s title or inspected properties.
							</p>
						)}
						<p>
							Provider values are separate from FFprobe inspection. Medal creation
							time is not a verified capture timestamp.
						</p>
						<dl>
							{fields
								.filter(([, value]) => value !== null)
								.map(([name, value]) => (
									<div key={name}>
										<dt>{name}</dt>
										<dd>{value}</dd>
									</div>
								))}
						</dl>
						{data.titleDifference && (
							<details>
								<summary>Medal title fields</summary>
								<p>
									These fields may differ. The title source above determines the
									displayed title.
								</p>
								<dl>
									<div>
										<dt>GameTitle</dt>
										<dd>{data.localTitle}</dd>
									</div>
									<div>
										<dt>Content title</dt>
										<dd>{data.contentTitle}</dd>
									</div>
								</dl>
							</details>
						)}
						{data.gameConflict && (
							<p>
								This entry supplies contradictory game names. No game name is
								selected.
							</p>
						)}
						{data.gameLookupAmbiguous && (
							<p>
								Game name unavailable: this category ID maps to multiple names in
								the catalog.
							</p>
						)}
						<details>
							<summary>Additional Medal metadata and provenance</summary>
							<dl>
								{entry.sourcePath && (
									<div>
										<dt>Catalog source</dt>
										<dd>{entry.sourcePath}</dd>
									</div>
								)}
								<div>
									<dt>Medal clip ID</dt>
									<dd>{entry.externalId}</dd>
								</div>
								<div>
									<dt>Imported</dt>
									<dd>{date(entry.importedMs)}</dd>
								</div>
								<div>
									<dt>Video reference</dt>
									<dd>{data.filePath || "None"}</dd>
								</div>
								{data.originalFilePath && (
									<div>
										<dt>Original reference</dt>
										<dd>{data.originalFilePath}</dd>
									</div>
								)}
								{data.image && (
									<div>
										<dt>Thumbnail source</dt>
										<dd>{data.image}</dd>
									</div>
								)}
							</dl>
							<Properties value={data.extra} />
						</details>
					</details>
				);
			})}
		</section>
	);
}
