import type { LibraryFolder, LibraryNavigation, LibraryQuery } from "../services/library";
import styles from "./LibraryCollections.module.scss";

function countLabel(count: number) {
	return count.toLocaleString() + (count === 1 ? " recording" : " recordings");
}

export function CollectionIcon({ folder = false }: { folder?: boolean }) {
	return (
		<svg
			viewBox="0 0 48 40"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.5"
			aria-hidden="true"
		>
			{folder ? (
				<>
					<path d="M4 12V7h14l5 5h21v23H4Z" />
					<path d="M4 17h40" />
				</>
			) : (
				<>
					<rect x="4" y="5" width="40" height="30" rx="3" />
					<path d="m20 13 12 7-12 7Z" />
				</>
			)}
		</svg>
	);
}

export function folderSelection(folder: LibraryFolder): Partial<LibraryQuery> {
	return { rootId: folder.rootId, folderId: folder.parentId ? folder.id : null, provider: null };
}

export function FolderTiles({
	folders,
	onSelect,
}: {
	folders: LibraryFolder[];
	onSelect: (changes: Partial<LibraryQuery>) => void;
}) {
	return (
		<div className={styles.tiles}>
			{folders.map((folder) => (
				<button
					key={folder.id}
					className={styles.tile}
					title={folder.path}
					onClick={() => onSelect(folderSelection(folder))}
				>
					<CollectionIcon folder />
					<span className={styles.identity}>
						<strong>{folder.name}</strong>
						<small>{countLabel(folder.recordings)} · including subfolders</small>
					</span>
				</button>
			))}
		</div>
	);
}

export function CollectionsHome({
	navigation,
	onSelect,
}: {
	navigation: LibraryNavigation;
	onSelect: (changes: Partial<LibraryQuery>) => void;
}) {
	const roots = navigation.folders.filter((folder) => folder.parentId === null);
	return (
		<div className={styles.home} aria-label="Collections homepage">
			<section aria-labelledby="all-collections-heading">
				<h2 id="all-collections-heading">Your library</h2>
				<div className={styles.tiles}>
					<button
						className={styles.tile}
						onClick={() => onSelect({ rootId: null, folderId: null, provider: null })}
					>
						<CollectionIcon />
						<span className={styles.identity}>
							<strong>All recordings</strong>
							<small>{countLabel(navigation.total)} · entire library</small>
						</span>
					</button>
				</div>
			</section>
			{navigation.sources.length > 0 && (
				<section aria-labelledby="source-collections-heading">
					<h2 id="source-collections-heading">Sources</h2>
					<div className={styles.tiles}>
						{navigation.sources.map((source) => (
							<button
								className={styles.tile}
								key={source.kind}
								onClick={() =>
									onSelect({
										rootId: null,
										folderId: null,
										provider: source.kind,
									})
								}
							>
								<CollectionIcon />
								<span className={styles.identity}>
									<strong>
										{source.kind === "medal" ? "Medal" : source.kind}
									</strong>
									<small>
										{source.recordings.toLocaleString()} associated{" "}
										{source.recordings === 1 ? "recording" : "recordings"}
									</small>
								</span>
							</button>
						))}
					</div>
				</section>
			)}
			<section aria-labelledby="folder-collections-heading">
				<h2 id="folder-collections-heading">Folders</h2>
				<FolderTiles folders={roots} onSelect={onSelect} />
				{!roots.length && (
					<p>
						No folders selected. Use Add folder to choose which recordings Klipper
						indexes.
					</p>
				)}
			</section>
			{navigation.total === 0 && (
				<section className={styles.empty}>
					<h2>Your recordings start here</h2>
					<p>Add a local folder or import Medal recordings to start browsing.</p>
				</section>
			)}
		</div>
	);
}

export function CollectionLocation({
	current,
	provider,
	folders,
	onHome,
	onSelect,
}: {
	current: LibraryFolder | null;
	provider: string | null;
	folders: LibraryFolder[];
	onHome: () => void;
	onSelect: (changes: Partial<LibraryQuery>) => void;
}) {
	const ancestors: LibraryFolder[] = [];
	let node = current;
	while (node && ancestors.length < folders.length) {
		ancestors.unshift(node);
		node = folders.find((folder) => folder.id === node?.parentId) ?? null;
	}
	return (
		<nav className={styles.location} aria-label="Collection location">
			<button onClick={onHome}>← Collections</button>
			<span aria-hidden="true">/</span>
			{current ? (
				ancestors.map((folder, index) => (
					<span className={styles.crumb} key={folder.id}>
						{index > 0 && <span aria-hidden="true">/</span>}
						<button
							title={folder.path}
							aria-current={folder.id === current.id ? "page" : undefined}
							onClick={() => onSelect(folderSelection(folder))}
						>
							{folder.name}
						</button>
					</span>
				))
			) : (
				<span aria-current="page">
					{provider === "medal" ? "Medal" : provider || "All recordings"}
				</span>
			)}
		</nav>
	);
}
