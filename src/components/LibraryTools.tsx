import { useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import type { LibraryQuery, LibraryStatus } from "../services/library";
import type { LibraryView } from "./LibraryBrowser";
import styles from "./Library.module.scss";

export function folderName(path: string) {
	return (
		path
			.replace(/[\\/]+$/, "")
			.split(/[\\/]/)
			.at(-1) || path
	);
}

export default function LibraryTools({
	query,
	search,
	view,
	status,
	count,
	busy,
	scanning,
	onSearch,
	onFilter,
	onView,
	onAdd,
	onRescan,
	onRemove,
}: {
	query: LibraryQuery;
	search: string;
	view: LibraryView;
	status: LibraryStatus | null;
	count: number;
	busy: boolean;
	scanning: boolean;
	onSearch: (value: string) => void;
	onFilter: (changes: Partial<LibraryQuery>) => void;
	onView: (view: LibraryView) => void;
	onAdd: () => void;
	onRescan: (id: number) => void;
	onRemove: (id: number) => void;
}) {
	const [panel, setPanel] = useState<"filters" | "folders" | null>(null);
	const filtersButton = useRef<HTMLButtonElement>(null);
	const foldersButton = useRef<HTMLButtonElement>(null);
	const filterCount =
		Number(query.rootId !== null) +
		Number(query.codec !== "all") +
		Number(query.filter !== "all");
	const roots = status?.roots ?? [];
	const selectedRoot = roots.find((root) => root.id === query.rootId);

	function toggle(next: "filters" | "folders") {
		setPanel((previous) => (previous === next ? null : next));
	}

	return (
		<div className={styles.tools}>
			<div className={styles.heading}>
				<div className={styles.title}>
					<h1>Library</h1>
					<span>
						{count.toLocaleString()} recordings{search || filterCount ? " found" : ""}
					</span>
				</div>
				<div className={styles.headingActions}>
					<button
						ref={foldersButton}
						aria-expanded={panel === "folders"}
						aria-controls="library-folders"
						onClick={() => toggle("folders")}
					>
						<svg viewBox="0 0 18 18" aria-hidden="true">
							<path d="M2 5V3h5l2 2h7v10H2z" />
						</svg>
						Folders
						{roots.length > 0 && <span className={styles.badge}>{roots.length}</span>}
					</button>
					<button
						className={styles.add}
						disabled={busy || scanning || !isTauri()}
						title={
							scanning
								? "Finish or cancel this scan before adding another folder"
								: undefined
						}
						onClick={onAdd}
					>
						<svg viewBox="0 0 18 18" aria-hidden="true">
							<path d="M9 3v12M3 9h12" />
						</svg>
						Add folder
					</button>
				</div>
			</div>
			<div className={styles.toolbar}>
				<label className={styles.search}>
					<svg viewBox="0 0 18 18" aria-hidden="true">
						<circle cx="7.5" cy="7.5" r="4.5" />
						<path d="m11 11 4 4" />
					</svg>
					<input
						type="search"
						aria-label="Search recordings"
						placeholder="Search recordings"
						value={search}
						maxLength={200}
						onChange={(event) => onSearch(event.target.value)}
					/>
				</label>
				<div className={styles.sort}>
					<label htmlFor="library-sort">Sort by</label>
					<select
						id="library-sort"
						aria-label="Sort recordings"
						value={query.sort}
						onChange={(event) => onFilter({ sort: event.target.value })}
					>
						<option value="date">Modified date</option>
						<option value="name">Name</option>
						<option value="duration">Duration</option>
						<option value="size">File size</option>
					</select>
					<button
						aria-label={query.descending ? "Sort ascending" : "Sort descending"}
						title={
							query.descending
								? "Descending; switch to ascending"
								: "Ascending; switch to descending"
						}
						onClick={() => onFilter({ descending: !query.descending })}
					>
						<svg viewBox="0 0 18 18" aria-hidden="true">
							<path
								d={
									query.descending
										? "M6 3v12m-3-3 3 3 3-3M11 4h4m-4 4h3m-3 4h2"
										: "M6 15V3M3 6l3-3 3 3m2 6h4m-4-4h3m-3-4h2"
								}
							/>
						</svg>
					</button>
				</div>
				<button
					ref={filtersButton}
					aria-expanded={panel === "filters"}
					aria-controls="library-filters"
					onClick={() => toggle("filters")}
				>
					<svg viewBox="0 0 18 18" aria-hidden="true">
						<path d="M3 5h12M5 9h8m-6 4h4" />
					</svg>
					Filters{filterCount > 0 && <span className={styles.badge}>{filterCount}</span>}
				</button>
				<div className={styles.viewSwitch} role="group" aria-label="Library view">
					<button
						aria-label="Grid view"
						title="Grid view"
						aria-pressed={view === "grid"}
						onClick={() => onView("grid")}
					>
						<svg viewBox="0 0 18 18" aria-hidden="true">
							<path d="M2.5 2.5h5v5h-5zM10.5 2.5h5v5h-5zM2.5 10.5h5v5h-5zM10.5 10.5h5v5h-5z" />
						</svg>
					</button>
					<button
						aria-label="List view"
						title="List view"
						aria-pressed={view === "list"}
						onClick={() => onView("list")}
					>
						<svg viewBox="0 0 18 18" aria-hidden="true">
							<path d="M2 3h4v4H2zM9 4h7M9 6h5M2 11h4v4H2zM9 12h7m-7 2h5" />
						</svg>
					</button>
				</div>
			</div>
			{filterCount > 0 && panel !== "filters" && (
				<div className={styles.filterSummary} aria-label="Active filters">
					{query.rootId !== null && (
						<span title={selectedRoot?.path}>
							Folder:{" "}
							{selectedRoot ? folderName(selectedRoot.path) : "Unavailable folder"}
						</span>
					)}
					{query.codec !== "all" && (
						<span>{query.codec === "h264" ? "H.264" : query.codec.toUpperCase()}</span>
					)}
					{query.filter !== "all" && <span>Status: {query.filter}</span>}
					<button onClick={() => onFilter({ rootId: null, filter: "all", codec: "all" })}>
						Clear filters
					</button>
				</div>
			)}
			<div
				id="library-filters"
				className={styles.filterPanel}
				hidden={panel !== "filters"}
				onKeyDown={(event) => {
					if (event.key === "Escape") {
						setPanel(null);
						filtersButton.current?.focus();
					}
				}}
			>
				<label>
					Folder
					<select
						aria-label="Folder source"
						value={query.rootId ?? ""}
						onChange={(event) =>
							onFilter({
								rootId: event.target.value ? Number(event.target.value) : null,
							})
						}
					>
						<option value="">All folders</option>
						{roots.map((root) => (
							<option key={root.id} value={root.id} title={root.path}>
								{root.path}
							</option>
						))}
					</select>
				</label>
				<label>
					Status
					<select
						aria-label="Indexing status"
						value={query.filter}
						onChange={(event) => onFilter({ filter: event.target.value })}
					>
						<option value="all">All statuses</option>
						<option value="available">Available</option>
						<option value="missing">Missing</option>
						<option value="pending">Pending analysis</option>
						<option value="error">Analysis errors</option>
					</select>
				</label>
				<label>
					Codec
					<select
						aria-label="Video codec"
						value={query.codec}
						onChange={(event) => onFilter({ codec: event.target.value })}
					>
						<option value="all">All codecs</option>
						<option value="hevc">HEVC</option>
						<option value="h264">H.264</option>
					</select>
				</label>
				<button
					disabled={!filterCount}
					onClick={() => onFilter({ rootId: null, filter: "all", codec: "all" })}
				>
					Clear filters
				</button>
			</div>
			<div
				id="library-folders"
				className={styles.folderPanel}
				hidden={panel !== "folders"}
				onKeyDown={(event) => {
					if (event.key === "Escape") {
						setPanel(null);
						foldersButton.current?.focus();
					}
				}}
			>
				<p>
					Indexed folders <span>Removing a folder keeps its recordings on disk.</span>
				</p>
				{!roots.length && (
					<p className={styles.secondary}>
						Add a folder to start your library. Only folders you choose are scanned.
					</p>
				)}
				<div className={styles.sourceList}>
					{roots.map((root) => (
						<div className={styles.source} key={root.id}>
							<div>
								<strong>{folderName(root.path)}</strong>
								<span className={styles.sourcePath}>{root.path}</span>
								<small>
									{root.recordings.toLocaleString()} recordings · {root.scanState}
									{root.lastScan
										? " · Last scan " + new Date(root.lastScan).toLocaleString()
										: ""}
								</small>
								{root.error && <small>{root.error}</small>}
							</div>
							<button
								disabled={busy || scanning}
								aria-label={"Rescan " + root.path}
								onClick={() => onRescan(root.id)}
							>
								Rescan
							</button>
							<button
								disabled={busy}
								aria-label={"Remove " + root.path}
								title="Remove from the library; source recordings stay on disk"
								onClick={() => onRemove(root.id)}
							>
								Remove
							</button>
						</div>
					))}
				</div>
			</div>
		</div>
	);
}
