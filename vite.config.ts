import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const privateDirectories = [
	"**/local/**",
	"**/localBAK/**",
	"**/.validation/**",
	"**/tools/**",
	"**/.venv/**",
	"**/.aws/**",
	"**/.ssh/**",
	"**/.codex/**",
	"**/.agents/**",
	"**/.cache/**",
	"**/.pytest_cache/**",
	"**/.mypy_cache/**",
	"**/.ruff_cache/**",
	"**/tmp/**",
	"**/temp/**",
];

export default defineConfig({
	plugins: [react()],
	server: {
		host: "127.0.0.1",
		port: 1420,
		strictPort: true,
		fs: {
			strict: true,
			// A custom deny list replaces Vite's defaults; retain their protections.
			deny: [
				".env",
				".env.*",
				"*.{crt,pem,key,p12,pfx,cer,der}",
				".npmrc",
				".yarnrc.yml",
				"**/.git/**",
				...privateDirectories,
			],
		},
		watch: { ignored: ["**/src-tauri/**", ...privateDirectories] },
	},
	clearScreen: false,
});
