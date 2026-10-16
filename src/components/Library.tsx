import { useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { bytes, time } from "../lib/format";
import { errorMessage } from "../services/media";
import {
	addLibraryFolder,
	cancelLibraryScan,
	libraryStatus,
	queryLibrary,
	removeLibraryFolder,
	rescanLibraryFolder,
	scanActive,
	type LibraryPage,
	type LibraryQuery,
	type LibraryStatus,
} from "../services/library";
import styles from "./Library.module.scss";

const initialQuery: LibraryQuery = {
	search: "",
	rootId: null,
	sort: "date",
	descending: true,
	filter: "all",
	codec: "all",
	offset: 0,
};

export default function Library({
	active,
	selecting,
	onOpen,
}: {
	active: boolean;
	selecting: boolean;
	onOpen: (id: number) => void;
}) {
	const [status, setStatus] = useState<LibraryStatus | null>(null);
	const [page, setPage] = useState<LibraryPage>({ items: [], total: 0, limit: 40 });
	const [query, setQuery] = useState(initialQuery);
	const [search, setSearch] = useState("");
	const [revision, setRevision] = useState(0);
	const [busy, setBusy] = useState(false);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState("");
	const list = useRef<HTMLDivElement>(null);
	const alive = useRef(false);
	const actionPending = useRef(false);
	const scanning = scanActive(status?.progress.phase ?? "");

	useEffect(() => {
		alive.current = true;
		return () => {
			alive.current = false;
		};
	}, []);

	useEffect(() => {
		const timer = setTimeout(() => {
			setQuery((previous) =>
				previous.search === search ? previous : { ...previous, search, offset: 0 },
			);
		}, 200);
		return () => clearTimeout(timer);
	}, [search]);

	useEffect(() => {
		if (!active) return;
		let disposed = false;
		let timer: ReturnType<typeof setTimeout>;
		async function load() {
			let delay = 2500;
			try {
				const [overview, result] = await Promise.all([
					libraryStatus(),
					queryLibrary(query),
				]);
				if (!disposed) {
					setStatus(overview);
					setPage(result);
					delay = scanActive(overview.progress.phase) ? 600 : 2500;
					if (query.offset > 0 && query.offset >= result.total) {
						setQuery((previous) => ({ ...previous, offset: 0 }));
					}
				}
			} catch (failure) {
				if (!disposed) setError(errorMessage(failure));
			} finally {
				if (!disposed) {
					setLoading(false);
					timer = setTimeout(() => void load(), delay);
				}
			}
		}
		void load();
		return () => {
			disposed = true;
			clearTimeout(timer);
		};
	}, [active, query, revision]);

	function filter(changes: Partial<LibraryQuery>) {
		setQuery((previous) => ({ ...previous, ...changes, offset: 0 }));
		if (list.current) list.current.scrollTop = 0;
	}

	async function action(run: () => Promise<unknown>) {
		if (actionPending.current) return;
		actionPending.current = true;
		setBusy(true);
		setError("");
		try {
			await run();
		} catch (failure) {
			if (alive.current) setError(errorMessage(failure));
		} finally {
			actionPending.current = false;
			if (alive.current) {
				setBusy(false);
				setRevision((value) => value + 1);
			}
		}
	}

	const progress = status?.progress;
	return (
		<section className={styles.library} hidden={!active} aria-label="Recording library">
			<div className={styles.heading}>
				<div>
					<h1>Library</h1>
					<p>Your local recordings, in one place.</p>
				</div>
				<button
					className={styles.add}
					disabled={busy || scanning || !isTauri()}
					title={
						scanning
							? "Finish or cancel this scan before adding another folder"
							: undefined
					}
					onClick={() => void action(addLibraryFolder)}
				>
					Add folder
				</button>
			</div>
			{error && (
				<p className={styles.error} role="alert">
					{error}
				</p>
			)}
			<div className={styles.filters}>
				<input
					type="search"
					aria-label="Search recordings"
					placeholder="Search filenames or paths"
					value={search}
					maxLength={200}
					onChange={(event) => {
						setSearch(event.target.value);
						if (list.current) list.current.scrollTop = 0;
					}}
				/>
				<select
					aria-label="Folder source"
					value={query.rootId ?? ""}
					onChange={(event) =>
						filter({ rootId: event.target.value ? Number(event.target.value) : null })
					}
				>
					<option value="">All folders</option>
					{status?.roots.map((root) => (
						<option key={root.id} value={root.id}>
							{root.path}
						</option>
					))}
				</select>
				<select
					aria-label="Sort recordings"
					value={query.sort}
					onChange={(event) => filter({ sort: event.target.value })}
				>
					<option value="date">Modified date</option>
					<option value="name">Name</option>
					<option value="duration">Duration</option>
					<option value="size">File size</option>
				</select>
				<button
					aria-label={query.descending ? "Sort ascending" : "Sort descending"}
					onClick={() => filter({ descending: !query.descending })}
				>
					{query.descending ? "Descending" : "Ascending"}
				</button>
				<select
					aria-label="Indexing status"
					value={query.filter}
					onChange={(event) => filter({ filter: event.target.value })}
				>
					<option value="all">All statuses</option>
					<option value="available">Available</option>
					<option value="missing">Missing</option>
					<option value="pending">Pending analysis</option>
					<option value="error">Analysis errors</option>
				</select>
				<select
					aria-label="Video codec"
					value={query.codec}
					onChange={(event) => filter({ codec: event.target.value })}
				>
					<option value="all">All codecs</option>
					<option value="hevc">HEVC</option>
					<option value="h264">H.264</option>
				</select>
			</div>
			{!!status?.roots.length && (
				<details className={styles.sources}>
					<summary>
						Folders <span>{status.roots.length}</span>
					</summary>
					<div className={styles.sourceList}>
						{status.roots.map((root) => (
							<div className={styles.source} key={root.id}>
								<div>
									<strong>{root.path}</strong>
									<small>
										{root.recordings} recordings · {root.scanState}
										{root.lastScan
											? " · Last scan " +
												new Date(root.lastScan).toLocaleString()
											: ""}
									</small>
									{root.error && <small>{root.error}</small>}
								</div>
								<button
									disabled={busy || scanning}
									aria-label={"Rescan " + root.path}
									onClick={() => void action(() => rescanLibraryFolder(root.id))}
								>
									Rescan
								</button>
								<button
									disabled={busy}
									aria-label={"Remove " + root.path}
									title="Remove this folder from the library. Recordings remain on disk."
									onClick={() =>
										void action(async () => {
											await removeLibraryFolder(root.id);
											if (alive.current && query.rootId === root.id)
												filter({ rootId: null });
										})
									}
								>
									Remove
								</button>
							</div>
						))}
					</div>
				</details>
			)}
			{progress?.phase && (
				<div className={styles.progress} role="status" aria-live="polite">
					<div>
						<strong>
							{scanning
								? progress.phase === "discovering"
									? "Discovering recordings"
									: "Inspecting recordings"
								: "Scan " + progress.phase}
						</strong>
						<span>
							{progress.discovered} found · {progress.analyzed} analyzed ·{" "}
							{progress.reused} unchanged
							{progress.skippedLinks
								? " · " + progress.skippedLinks + " links skipped"
								: ""}
						</span>
					</div>
					{scanning && (
						<button disabled={busy} onClick={() => void action(cancelLibraryScan)}>
							Cancel scan
						</button>
					)}
					{!!progress.issueCount && (
						<details>
							<summary>{progress.issueCount} scan issues</summary>
							<ul>
								{progress.issues.map((issue, index) => (
									<li key={index}>{issue}</li>
								))}
							</ul>
							{progress.issueCount > progress.issues.length && (
								<p>Showing the first {progress.issues.length} issues.</p>
							)}
						</details>
					)}
				</div>
			)}
			<div
				ref={list}
				className={styles.list}
				aria-label="Library recordings"
				tabIndex={0}
				aria-busy={loading}
			>
				{!page.items.length ? (
					<div className={styles.empty}>
						<h2>
							{loading
								? "Loading library…"
								: status?.roots.length
									? "No matching recordings"
									: "No folders added"}
						</h2>
						<p>
							{status?.roots.length
								? "Try another filter or rescan a folder to update its recordings."
								: "Add a recording folder to discover its videos. Source files stay untouched."}
						</p>
						{!isTauri() && <p>Open the desktop app to choose local folders.</p>}
					</div>
				) : (
					<ul>
						{page.items.map((item) => (
							<li key={item.id}>
								<button
									className={styles.recording}
									disabled={selecting || item.availability !== "available"}
									aria-label={"Play " + item.name}
									onClick={() => onOpen(item.id)}
								>
									<span className={styles.play} aria-hidden="true">
										▷
									</span>
									<span className={styles.identity}>
										<strong>{item.name}</strong>
										<span className={styles.path}>{item.path}</span>
										{(item.availability !== "available" ||
											item.status === "error") && (
											<span className={styles.problem}>
												{item.error ?? item.availability}
											</span>
										)}
									</span>
									<span className={styles.media}>
										<span>
											{item.duration === null
												? item.status === "pending"
													? "Pending analysis"
													: "—"
												: time(item.duration)}
										</span>
										<small>
											{[
												item.width && item.height
													? item.width + " × " + item.height
													: null,
												item.videoCodec?.toUpperCase(),
											]
												.filter(Boolean)
												.join(" · ")}
										</small>
									</span>
									<span className={styles.file}>
										<span>{bytes(item.size)}</span>
										<small>
											{item.modifiedMs === null
												? "Date unavailable"
												: new Date(item.modifiedMs).toLocaleDateString()}
										</small>
									</span>
								</button>
							</li>
						))}
					</ul>
				)}
			</div>
			<footer className={styles.footer}>
				<span>
					{page.total} recordings
					{page.total > 0
						? " · " +
							(query.offset + 1) +
							"–" +
							Math.min(query.offset + page.limit, page.total)
						: ""}
				</span>
				<div>
					<button
						disabled={query.offset === 0}
						onClick={() => {
							setQuery((previous) => ({
								...previous,
								offset: Math.max(0, previous.offset - page.limit),
							}));
							if (list.current) list.current.scrollTop = 0;
						}}
					>
						Previous
					</button>
					<button
						disabled={query.offset + page.limit >= page.total}
						onClick={() => {
							setQuery((previous) => ({
								...previous,
								offset: previous.offset + page.limit,
							}));
							if (list.current) list.current.scrollTop = 0;
						}}
					>
						Next
					</button>
				</div>
			</footer>
		</section>
	);
}
