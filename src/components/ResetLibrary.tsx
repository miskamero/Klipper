import { useEffect, useRef } from "react";
import styles from "./MedalImport.module.scss";
import libraryStyles from "./Library.module.scss";

export default function ResetLibrary({
	busy,
	onCancel,
	onConfirm,
}: {
	busy: boolean;
	onCancel: () => void;
	onConfirm: () => void;
}) {
	const dialog = useRef<HTMLDialogElement>(null);
	const cancel = useRef<HTMLButtonElement>(null);
	useEffect(() => {
		const element = dialog.current;
		element?.showModal();
		cancel.current?.focus();
		return () => element?.close();
	}, []);
	return (
		<dialog
			ref={dialog}
			className={styles.dialog}
			aria-labelledby="reset-library-title"
			onCancel={(event) => {
				event.preventDefault();
				if (!busy) onCancel();
			}}
		>
			<h2 id="reset-library-title">Reset Library?</h2>
			<p>
				This will forget all indexed recordings, selected folders, imported metadata, and
				cached thumbnails.
			</p>
			<p>
				Your original video recordings and recording application files will not be deleted.
			</p>
			{busy && <p role="status">Stopping library work and clearing the library…</p>}
			<footer>
				<button ref={cancel} disabled={busy} onClick={onCancel}>
					Cancel
				</button>
				<button className={libraryStyles.destructive} disabled={busy} onClick={onConfirm}>
					Reset Library
				</button>
			</footer>
		</dialog>
	);
}
