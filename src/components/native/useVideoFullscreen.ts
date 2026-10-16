import { useCallback, useEffect, useRef, useState } from "react";

export default function useVideoFullscreen({
	fullscreen,
	pointer,
	exit,
	onChange,
	focusControls,
}: {
	fullscreen: boolean;
	pointer?: { x: number; y: number } | null;
	exit: () => void;
	onChange?: (value: boolean) => void;
	focusControls?: () => void;
}) {
	const transport = useRef<HTMLDivElement>(null);
	const fullscreenButton = useRef<HTMLButtonElement>(null);
	const [controlsVisible, setControlsVisible] = useState(true);
	const idle = useRef<ReturnType<typeof setTimeout>>(undefined);
	const keyboard = useRef(false);
	const previousFullscreen = useRef(false);
	const wake = useCallback((y?: number) => {
		clearTimeout(idle.current);
		setControlsVisible(true);
		const rect = transport.current?.getBoundingClientRect();
		const top = rect?.height ? rect.top : innerHeight - 64;
		if (!keyboard.current && !(y !== undefined && y >= top))
			idle.current = setTimeout(() => setControlsVisible(false), 500);
	}, []);
	const wakeControls = useCallback(
		(usingKeyboard: boolean) => {
			keyboard.current = usingKeyboard;
			wake();
		},
		[wake],
	);
	useEffect(() => {
		onChange?.(fullscreen);
		if (previousFullscreen.current && !fullscreen)
			fullscreenButton.current?.focus({ preventScroll: true });
		previousFullscreen.current = fullscreen;
		return () => onChange?.(false);
	}, [fullscreen, onChange]);
	useEffect(() => {
		if (fullscreen) {
			keyboard.current = false;
			wake(pointer?.y);
		}
	}, [fullscreen, pointer?.x, pointer?.y, wake]);
	useEffect(() => {
		if (!fullscreen) return;
		function keydown(event: KeyboardEvent) {
			if (event.key === "Escape") {
				event.preventDefault();
				exit();
			} else if (event.key === "Tab") {
				keyboard.current = true;
				wake();
				if (focusControls) {
					event.preventDefault();
					focusControls();
				} else if (!controlsVisible) {
					event.preventDefault();
					requestAnimationFrame(() =>
						transport.current?.querySelector("button")?.focus(),
					);
				}
			}
		}
		window.addEventListener("keydown", keydown);
		return () => window.removeEventListener("keydown", keydown);
	}, [fullscreen, controlsVisible, exit, wake, focusControls]);
	useEffect(() => () => clearTimeout(idle.current), []);
	return {
		transport,
		fullscreenButton,
		controlsVisible: !fullscreen || controlsVisible,
		wakeControls,
		onPointerMove: (event: React.PointerEvent) => {
			if (fullscreen) {
				keyboard.current = false;
				wake(event.clientY);
			}
		},
	};
}
