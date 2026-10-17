import { useEffect, useRef, useState } from "react";
import {
	cancelMedalImport,
	commitMedalImport,
	medalImportProgress,
	medalPreviewPage,
	selectMedalCatalog,
	type ImportPreview,
	type ImportProgress,
} from "../services/medal";
import { errorMessage } from "../services/media";
import styles from "./MedalImport.module.scss";

const countLabels: Record<string, string> = {
	newFiles: "New references",
	conflicts: "Contradictory metadata references",
	warnings: "Game lookup warnings",
	recovered: "Newly enriched recordings",
	identityConflicts: "Identity conflicts",
	metadataDifferences: "References with source differences",
	timestampWarnings: "Records with unavailable timestamps",
	invalidIdentities: "Skipped invalid identities",
};
function claim(value: unknown): string {
	if (Array.isArray(value)) return value.map(claim).join(", ");
	return value === null
		? "Unknown"
		: typeof value === "boolean"
			? value
				? "Yes"
				: "No"
			: String(value);
}

export default function MedalImport({
	onClose,
	onImported,
}: {
	onClose: () => void;
	onImported: () => void;
}) {
	const dialog = useRef<HTMLDialogElement>(null);
	const alive = useRef(false);
	const running = useRef(false);
	const cancelled = useRef(false);
	const [preview, setPreview] = useState<ImportPreview | null>(null);
	const [progress, setProgress] = useState<ImportProgress | null>(null);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const [includeNew, setIncludeNew] = useState(false);
	const [complete, setComplete] = useState(false);
	useEffect(() => {
		alive.current = true;
		dialog.current?.showModal();
		return () => {
			alive.current = false;
			void cancelMedalImport().catch(() => {});
		};
	}, []);
	useEffect(() => {
		if (!busy) return;
		let disposed = false;
		let timer: ReturnType<typeof setTimeout>;
		async function poll() {
			try {
				const status = await medalImportProgress();
				if (!disposed) setProgress(status);
			} catch {
				/* The operation reports its own errors. */
			}
			if (!disposed) timer = setTimeout(() => void poll(), 250);
		}
		void poll();
		return () => {
			disposed = true;
			clearTimeout(timer);
		};
	}, [busy]);
	async function work(action: () => Promise<void>) {
		if (running.current) return;
		running.current = true;
		cancelled.current = false;
		setBusy(true);
		setError("");
		try {
			await action();
		} catch (reason) {
			if (alive.current) {
				if (cancelled.current) {
					setPreview(null);
					setProgress({ phase: "cancelled", done: 0, total: 0, linked: 0, error: "" });
				} else setError(errorMessage(reason));
			}
		} finally {
			running.current = false;
			if (alive.current) setBusy(false);
		}
	}
	async function close() {
		cancelled.current = true;
		await cancelMedalImport().catch(() => {});
		if (!running.current) onClose();
	}
	return (
		<dialog
			ref={dialog}
			className={styles.dialog}
			aria-labelledby="medal-import-title"
			onCancel={(event) => {
				event.preventDefault();
				void close();
			}}
		>
			<header>
				<h2 id="medal-import-title">Import from Medal</h2>
				<button onClick={() => void close()}>{busy ? "Cancel operation" : "Close"}</button>
			</header>
			<p>
				Choose Medal’s local clips.json catalog or SQLite .db database. Preview the
				associations before saving. Medal does not need to be running.
			</p>
			{error && <p role="alert">{error}</p>}
			{!busy && progress?.phase === "cancelled" && (
				<p role="status">Import cancelled. No changes were saved.</p>
			)}
			{busy && (
				<p role="status">
					{progress?.phase === "previews"
						? "Metadata saved · Caching previews"
						: progress?.phase === "importing"
							? "Importing"
							: "Preparing preview"}
					… {progress?.done ?? 0} / {progress?.total || "—"}
				</p>
			)}
			{complete ? (
				<p role="status">
					Import complete. {progress?.linked ?? 0} recording associations saved.
					Unresolved catalog records are retained without attaching guessed metadata.
					{progress?.warning && <span> {progress.warning}</span>}
				</p>
			) : (
				preview && (
					<>
						<p>
							{preview.sourceFormat === "sqlite"
								? "Medal SQLite database"
								: "Medal JSON catalog"}
							{preview.sourcePath && (
								<>
									{" "}
									· <span>{preview.sourcePath}</span>
								</>
							)}
						</p>
						<p>
							{preview.catalogEntries.toLocaleString()} catalog entries ·{" "}
							{preview.total.toLocaleString()} video references
						</p>
						<dl className={styles.counts}>
							{Object.entries(preview.counts)
								.filter(
									([label, count]) =>
										count > 0 ||
										!["timestampWarnings", "invalidIdentities"].includes(label),
								)
								.map(([label, count]) => (
									<div key={label}>
										<dt>{countLabels[label] ?? label}</dt>
										<dd>{count.toLocaleString()}</dd>
									</div>
								))}
						</dl>
						<p>
							Only exact paths or verified file identities are matched. Ambiguous
							references stay unresolved. Titles from edits are not applied to
							originals.
						</p>
						{preview.counts.conflicts > 0 && (
							<p>
								Some references contain contradictory metadata or changed file
								identities. Existing selected values are retained; new unresolved
								titles use the filename fallback. Imported values remain
								inspectable.
							</p>
						)}
						{preview.counts.warnings > 0 && (
							<p>
								Some game IDs map to multiple names and have no direct game name.
								Their game names remain unknown; titles are unaffected.
							</p>
						)}
						{!!preview.counts.timestampWarnings && (
							<p>
								Some optional provider timestamps are unavailable. Valid metadata
								can still be imported. Previously imported valid timestamps are
								retained and labeled in the inspector.
							</p>
						)}
						{!!preview.counts.invalidIdentities && (
							<p>
								Entries without a usable local provider identity were skipped. No
								identity or recording association was invented.
							</p>
						)}
						{!!preview.counts.metadataDifferences && (
							<p>
								Sources disagree on some fields. Existing selected values are
								preserved; both claims remain inspectable. These differences do not
								imply incorrect file associations.
							</p>
						)}
						{preview.counts.newFiles > 0 && (
							<label className={styles.approval}>
								<input
									type="checkbox"
									checked={includeNew}
									disabled={busy}
									onChange={(e) => setIncludeNew(e.target.checked)}
								/>
								Add all {preview.counts.newFiles} new video references shown in this
								preview. No videos are copied and no folders are scanned.
							</label>
						)}
						<div
							className={styles.rows}
							aria-label="Medal import references"
							aria-busy={busy}
						>
							{preview.rows.map((row) => (
								<div key={row.index} className={styles.row}>
									<strong>
										{row.title ||
											row.path?.split(/[\\/]/).at(-1) ||
											"No local recording"}
									</strong>
									<span>
										{row.status} · {row.relation}
										{row.game ? " · " + row.game : ""}
										{row.conflict ? " · Metadata conflict" : ""}
										{row.warning ? " · Game lookup unresolved" : ""}
										{row.identityConflict ? " · File identity conflict" : ""}
									</span>
									<details>
										<summary>File and provider identity</summary>
										<p>{row.path || "No local video path"}</p>
										<p>Medal ID: {row.clipId}</p>
									</details>
									{!!row.timestampWarnings?.length && (
										<details>
											<summary>Unavailable timestamps</summary>
											<ul>
												{row.timestampWarnings.map((warning) => (
													<li key={warning}>{warning}</li>
												))}
											</ul>
										</details>
									)}
									{!!row.differences?.length && (
										<details>
											<summary>
												Source metadata differences (
												{row.differences.length})
											</summary>
											<dl>
												{row.differences.map((difference, index) => (
													<div key={index}>
														<dt>{difference.field}</dt>
														<dd>
															Existing {difference.existingFormat}:{" "}
															{claim(difference.existing)}
															<br />
															Incoming{" "}
															{preview.sourceFormat ?? "json"}:{" "}
															{claim(difference.incoming)}
														</dd>
													</div>
												))}
											</dl>
										</details>
									)}
								</div>
							))}
						</div>
						<div className={styles.pagination}>
							<button
								disabled={busy || preview.offset === 0}
								onClick={() =>
									void work(async () => {
										const next = await medalPreviewPage(
											preview.token,
											Math.max(0, preview.offset - 50),
										);
										if (alive.current) setPreview(next);
									})
								}
							>
								Previous references
							</button>
							<span>
								{preview.total ? preview.offset + 1 : 0}–
								{Math.min(preview.offset + 50, preview.total)} of {preview.total}
							</span>
							<button
								disabled={busy || preview.offset + 50 >= preview.total}
								onClick={() =>
									void work(async () => {
										const next = await medalPreviewPage(
											preview.token,
											preview.offset + 50,
										);
										if (alive.current) setPreview(next);
									})
								}
							>
								Next references
							</button>
						</div>
					</>
				)
			)}
			<footer>
				{!complete && (
					<button
						disabled={busy}
						onClick={() =>
							void work(async () => {
								setPreview(null);
								setIncludeNew(false);
								setProgress(null);
								const next = await selectMedalCatalog();
								if (alive.current) setPreview(next);
							})
						}
					>
						Choose catalog
					</button>
				)}
				{preview && !complete && (
					<button
						disabled={busy}
						onClick={() =>
							void work(async () => {
								const result = await commitMedalImport(preview.token, includeNew);
								if (alive.current) {
									setProgress(result);
									setComplete(true);
									onImported();
								}
							})
						}
					>
						{includeNew
							? "Import metadata and approved references"
							: "Import metadata only"}
					</button>
				)}
				{complete && <button onClick={onClose}>Done</button>}
			</footer>
		</dialog>
	);
}
