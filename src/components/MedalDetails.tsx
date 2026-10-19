import type { ImportedMetadata } from "../services/medal";
import { time } from "../lib/format";
import { commonClaim, localizedDate, present } from "../lib/inspector";
import { CopyValue, MetadataValues, PropertyRows } from "./InspectorValues";
import styles from "./Inspector.module.scss";

export default function MedalDetails({ entries }: { entries: ImportedMetadata[] }) {
	const primary = entries.filter((entry) => entry.relation === "primary");
	const fields = [
		{ label: "Description", values: primary.map((entry) => entry.data.description) },
		{ label: "Tags", values: primary.map((entry) => entry.data.tags) },
		{
			label: "Local Medal creation",
			values: primary.map((entry) => entry.data.createdMs),
			kind: "date",
		},
		{
			label: "Remote content creation",
			values: primary.map((entry) => entry.data.contentCreatedMs),
			kind: "date",
		},
		{ label: "Last edit", values: primary.map((entry) => entry.data.lastEditMs), kind: "date" },
		{
			label: "Provider duration",
			values: primary.map((entry) => entry.data.duration),
			kind: "duration",
		},
		{ label: "Bookmarks", values: primary.map((entry) => entry.data.extra.bookmarks) },
		{ label: "Trim information", values: primary.map((entry) => entry.data.extra.trim) },
		{ label: "Trim history", values: primary.map((entry) => entry.data.extra.trimHistory) },
		{ label: "Event", values: primary.map((entry) => entry.data.extra.event) },
		{
			label: "Edited audio",
			values: primary.map((entry) => entry.data.extra.editAudioStreams),
		},
	];
	const claims = fields.map((field) => ({ ...field, ...commonClaim(field.values) }));
	const relationships = entries.filter((entry) => entry.relation === "original");
	return (
		<>
			<PropertyRows
				rows={claims
					.filter((field) => present(field.value))
					.map((field) => [
						field.label,
						field.kind === "date" ? (
							localizedDate(field.value)
						) : field.kind === "duration" && typeof field.value === "number" ? (
							time(field.value)
						) : (
							<MetadataValues value={field.value} />
						),
					])}
			/>
			{primary.some(
				(entry) => entry.data.createdMs != null || entry.data.contentCreatedMs != null,
			) && (
				<p className={styles.note}>
					Medal creation times are provider dates, not verified capture dates.
				</p>
			)}
			{primary.some((entry) => entry.data.originalFilePath) && (
				<p>
					Medal supplies an explicit original-recording reference for this edit. Its path
					is available in Advanced &amp; source data.
				</p>
			)}
			{relationships.map((entry, index) => (
				<p key={index}>
					Medal explicitly links this recording to a related edit
					{entry.data.title ? `: ${entry.data.title}` : "."}
				</p>
			))}
			{claims.some((field) => field.differs) && (
				<p>
					Some provider fields differ between sources. Compare the imported claims in
					Advanced &amp; source data.
				</p>
			)}
			{!claims.some((field) => present(field.value) || field.differs) &&
				relationships.length === 0 && (
					<p>
						No additional descriptive Medal information is available. Import references
						remain in Advanced &amp; source data.
					</p>
				)}
		</>
	);
}

// Every sanitized source claim remains inspectable, including selected historical values.
export function ProviderSourceDetails({ entries }: { entries: ImportedMetadata[] }) {
	return (
		<>
			{entries.map((entry, index) => {
				const data = entry.data;
				const format =
					data.sourceFormat === "sqlite"
						? "SQLite"
						: !data.sourceFormat || data.sourceFormat === "json"
							? "JSON"
							: data.sourceFormat;
				const exactTimes = {
					localCreationMs: data.createdMs,
					remoteContentCreationMs: data.contentCreatedMs,
					lastEditMs: data.lastEditMs,
					importedMs: entry.importedMs,
				};
				return (
					<details
						className={styles.source}
						key={`${entry.externalId}:${entry.relation}:${index}`}
					>
						<summary>
							{entry.provider !== "medal" ? `${entry.provider} · ` : ""}
							{format} source{entries.length > 1 ? ` ${index + 1}` : ""}
							{entry.relation === "original" ? " · Related edit" : ""}
						</summary>
						{entry.relation === "original" && (
							<p>
								These claims describe a related edit, not this original recording.
							</p>
						)}
						<PropertyRows
							rows={[
								["Provider", entry.provider],
								[
									"Provider entry ID",
									<CopyValue
										value={entry.externalId}
										label="provider entry ID"
									/>,
								],
								[
									"Local clip ID",
									<CopyValue
										value={data.localId || entry.externalId}
										label="local clip ID"
									/>,
								],
								[
									"Remote clip ID",
									data.remoteId ? (
										<CopyValue value={data.remoteId} label="remote clip ID" />
									) : null,
								],
								[
									"Game ID",
									data.gameId ? (
										<CopyValue value={data.gameId} label="game ID" />
									) : null,
								],
								[
									"Catalog",
									entry.sourcePath ? (
										<CopyValue value={entry.sourcePath} label="catalog path" />
									) : null,
								],
							]}
						/>
						{present(entry.selectedValues) && (
							<details>
								<summary>Selected library values</summary>
								<p>
									Existing selections survive unresolved source disagreements. A
									retained selection can differ from the latest source claim. User
									overrides take priority.
								</p>
								<MetadataValues value={entry.selectedValues} exact />
							</details>
						)}
						<details>
							<summary>Imported source claims</summary>
							<MetadataValues
								exact
								value={{
									title: data.title,
									localTitle: data.localTitle,
									contentTitle: data.contentTitle,
									titleField: data.titleField,
									contentHasTitle: data.contentHasTitle,
									titleDifference: data.titleDifference,
									description: data.description,
									game: data.game,
									gameField: data.gameField,
									gameConflict: data.gameConflict,
									gameLookupAmbiguous: data.gameLookupAmbiguous,
									durationSeconds: data.duration,
									tags: data.tags,
									remoteTrimStartSeconds: data.remoteTrimStart,
									relation: entry.relation,
								}}
							/>
						</details>
						<details>
							<summary>Exact timestamps</summary>
							<p>
								Normalized timestamps are Unix milliseconds. Other original values
								retain their source names and units.
							</p>
							<MetadataValues value={exactTimes} exact />
							<PropertyRows
								rows={Object.entries(exactTimes)
									.filter(([, value]) => localizedDate(value))
									.map(([label, value]) => [
										label + " (UTC)",
										new Date(value!).toISOString(),
									])}
							/>
						</details>
						{data.timestampWarnings?.map((warning) => (
							<p key={warning}>{warning}</p>
						))}
						{!!data.retainedTimestamps?.length && (
							<p>
								Previously imported valid timestamps retained:{" "}
								{data.retainedTimestamps.join(", ")}. The latest source did not
								supply usable replacements.
							</p>
						)}
						{[
							data.filePath,
							data.image,
							data.originalFilePath,
							data.originalImage,
						].some(present) && (
							<details>
								<summary>File and image references</summary>
								<PropertyRows
									rows={[
										[
											"Video",
											data.filePath ? (
												<CopyValue
													value={data.filePath}
													label="provider video path"
												/>
											) : null,
										],
										[
											"Thumbnail",
											data.image ? (
												<CopyValue
													value={data.image}
													label="provider thumbnail path"
												/>
											) : null,
										],
										[
											"Original recording",
											data.originalFilePath ? (
												<CopyValue
													value={data.originalFilePath}
													label="original recording path"
												/>
											) : null,
										],
										[
											"Original thumbnail",
											data.originalImage ? (
												<CopyValue
													value={data.originalImage}
													label="original thumbnail path"
												/>
											) : null,
										],
									]}
								/>
							</details>
						)}
						{present(data.extra) && (
							<details>
								<summary>Additional source fields</summary>
								<MetadataValues value={data.extra} exact />
							</details>
						)}
					</details>
				);
			})}
		</>
	);
}
