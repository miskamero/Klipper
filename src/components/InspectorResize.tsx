import { useEffect, useState, type RefObject } from "react";

const storageKey = "klipper.inspectorWidth";
const clamp = (width: number, maximum: number) => Math.min(maximum, Math.max(340, width));

export function useInspectorWidth(workspace: RefObject<HTMLDivElement | null>, active = true) {
	const [preferred, setPreferred] = useState(() => {
		try {
			const stored = Number(localStorage.getItem(storageKey));
			return stored >= 320 && stored <= 560 ? Math.max(340, stored) : 400;
		} catch {
			return 400;
		}
	});
	const [maximum, setMaximum] = useState(560);
	useEffect(() => {
		const element = workspace.current;
		if (!active || !element || typeof ResizeObserver === "undefined") return;
		const observer = new ResizeObserver(() => {
			setMaximum(Math.max(340, Math.min(560, element.clientWidth - 496)));
		});
		observer.observe(element);
		return () => observer.disconnect();
	}, [workspace, active]);
	useEffect(() => {
		try {
			localStorage.setItem(storageKey, String(preferred));
		} catch {
			/* Storage is optional. */
		}
	}, [preferred]);
	return {
		width: clamp(preferred, maximum),
		maximum,
		setWidth: (width: number) => setPreferred(clamp(width, maximum)),
	};
}

export default function InspectorResize({
	width,
	maximum,
	onResize,
	className,
}: {
	width: number;
	maximum: number;
	onResize: (width: number) => void;
	className: string;
}) {
	return (
		<div
			role="separator"
			aria-label="Resize recording details"
			aria-orientation="vertical"
			aria-controls="recording-inspector"
			aria-valuemin={340}
			aria-valuemax={maximum}
			aria-valuenow={Math.round(width)}
			aria-valuetext={`${Math.round(width)} pixels`}
			tabIndex={0}
			className={className}
			title="Drag to resize. Left/Right arrows adjust width."
			onPointerDown={(event) => {
				if (event.button === 0) {
					event.preventDefault();
					event.currentTarget.focus();
					event.currentTarget.setPointerCapture(event.pointerId);
				}
			}}
			onPointerMove={(event) => {
				if (event.currentTarget.hasPointerCapture(event.pointerId)) {
					const right = event.currentTarget.parentElement!.getBoundingClientRect().right;
					onResize(right - event.clientX - 8);
				}
			}}
			onPointerUp={(event) => {
				if (event.currentTarget.hasPointerCapture(event.pointerId))
					event.currentTarget.releasePointerCapture(event.pointerId);
			}}
			onKeyDown={(event) => {
				const step = event.shiftKey ? 32 : 16;
				const next =
					event.key === "ArrowLeft"
						? width + step
						: event.key === "ArrowRight"
							? width - step
							: event.key === "Home"
								? 340
								: event.key === "End"
									? maximum
									: null;
				if (next !== null) {
					event.preventDefault();
					onResize(next);
				}
			}}
		/>
	);
}
