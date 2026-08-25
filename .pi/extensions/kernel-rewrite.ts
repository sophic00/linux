/**
 * Kernel Rust Rewrite — custom tools for the pi agent team.
 *
 * Registers domain tools so agents don't hand-roll shell incantations:
 *   kernel_checkpatch — run scripts/checkpatch.pl on a commit range/patch
 *   kernel_build      — incremental out-of-tree build (x86_64/arm64, LLVM=1)
 *   kunit_run         — run KUnit suites with layered kunitconfigs
 *   safety_audit      — enforce // SAFETY: comments on unsafe blocks
 *   get_maintainer    — MAINTAINERS lookup for review routing
 *   tracker_update    — update rewrite/TRACKER.md rows safely
 *
 * Place in .pi/extensions/ (project-local). Loaded after project trust.
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { withFileMutationQueue } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { basename, dirname, join, resolve } from "node:path";
import os from "node:os";

const BUILD_TIMEOUT_MS = 45 * 60 * 1000;
const SHORT_TIMEOUT_MS = 5 * 60 * 1000;

function fail(msg: string): never {
	throw new Error(msg);
}

export default function kernelRewriteTools(pi: ExtensionAPI) {
	// ---- shared helpers -----------------------------------------------------

	async function sh(
		cwd: string,
		script: string,
		timeout = BUILD_TIMEOUT_MS,
		signal?: AbortSignal,
	): Promise<string> {
		const r = await pi.exec("bash", ["-c", script], { cwd, timeout, signal });
		if (r.code !== 0) {
			fail(
				`exit ${r.code}\n$ ${script}\n--- stdout ---\n${r.stdout?.slice(-4000)}\n--- stderr ---\n${r.stderr?.slice(-4000)}`,
			);
		}
		return `${r.stdout ?? ""}${r.stderr ? `\n[stderr]\n${r.stderr}` : ""}`;
	}

	// ---- tools ---------------------------------------------------------------

	pi.registerTool({
		name: "kernel_checkpatch",
		label: "Checkpatch",
		description:
			"Run Linux checkpatch.pl --strict on a git commit range (e.g. 'HEAD~3..HEAD') or patch file. Mandatory before any handoff.",
		promptSnippet: "Kernel patch style checking via checkpatch.pl",
		promptGuidelines: [
			"Use kernel_checkpatch before marking any port ready; fix all reports including warnings.",
		],
		parameters: Type.Object({
			target: Type.String({ description: "Git range or path to patch file" }),
			cwd: Type.Optional(Type.String()),
			strict: Type.Optional(Type.Boolean({ description: "Default true (--strict)" })),
		}),
		async execute(_id, p, signal, _onUpdate, ctx) {
			const root = resolve(ctx.cwd ?? process.cwd(), p.cwd ?? ".");
			const flag = p.strict === false ? "" : "--strict ";
			const out = await sh(root, `./scripts/checkpatch.pl ${flag}${p.target}`, SHORT_TIMEOUT_MS, signal);
			return { content: [{ type: "text", text: out.slice(-6000) }], details: {} };
		},
	});

	pi.registerTool({
		name: "kernel_build",
		label: "Kernel Build",
		description:
			"Incremental out-of-tree kernel build with LLVM=1 in a per-worktree O= dir. Reuses an existing .config via olddefconfig unless an explicit config target is given (defconfig seeded when absent). rust=true forces CONFIG_RUST=y. Reports tail of output.",
		promptSnippet: "Compile the kernel (out-of-tree, x86_64/arm64)",
		promptGuidelines: [
			"Use kernel_build instead of raw make so every agent gets its own build directory per arch.",
			"Pass rust=true for any build that must compile Rust code (driver ports).",
		],
		parameters: Type.Object({
			arch: Type.Optional(
				Type.Union([Type.Literal("x86_64"), Type.Literal("arm64")]),
			),
			config: Type.Optional(Type.String({ description: "defconfig|allmodconfig|... (default defconfig)" })),
			werror: Type.Optional(Type.Boolean({ description: "Pass WERROR=1 (default true)" })),
			rust: Type.Optional(Type.Boolean({ description: "Force CONFIG_RUST=y before building (default false)" })),
			jobs: Type.Optional(Type.Number()),
		}),
		async execute(_id, p, signal, onUpdate, ctx) {
			const root = ctx.cwd ?? process.cwd();
			const arch = p.arch ?? "x86_64";
			// Per-worktree O= dir so parallel agents in separate worktrees never
			// race on shared kbuild state.
			let slug = "default";
			try {
				slug =
					basename(resolve(root))
						.toLowerCase()
						.replace(/[^a-z0-9]+/g, "-")
						.replace(/^-+|-+$/g, "") || "default";
			} catch {
				/* keep default */
			}
			const oDir = `/tmp/rw-build-${arch}-${slug}`;
			const kArch = arch === "arm64" ? "arm64" : "";
			const jobs = Math.max(1, Math.min(8, Math.floor(p.jobs ?? os.cpus().length)));
			const werror = p.werror === false ? "" : "WERROR=1";
			const mk = (target: string, timeout = SHORT_TIMEOUT_MS) =>
				sh(
					root,
					`make LLVM=1 ${kArch ? `ARCH=${kArch} ` : ""}O=${oDir} ${target}`.trimEnd(),
					timeout,
					signal,
				);
			let log = "";
			// Configure step: explicit config target always wins; otherwise reuse
			// an existing .config via olddefconfig (incremental-friendly), seeding
			// defconfig only when none exists yet.
			if (p.config && p.config !== "olddefconfig") {
				log += await mk(p.config);
			} else {
				const probe = await pi.exec(
					"bash",
					["-c", `test -f '${oDir}/.config' && echo y || echo n`],
					{ cwd: root, timeout: SHORT_TIMEOUT_MS },
				);
				if (!probe.stdout?.includes("y")) log += await mk("defconfig");
			}
			if (p.rust === true) {
				log += await sh(
					root,
					`./scripts/config --file '${oDir}/.config' -e RUST`,
					SHORT_TIMEOUT_MS,
					signal,
				);
			}
			log += await mk("olddefconfig");
			onUpdate?.({
				content: [{ type: "text", text: `configured ${arch}; compiling…` }],
				details: {},
			});
			log += await mk(`${werror} -j${jobs}`.trim(), BUILD_TIMEOUT_MS);
			return {
				content: [{ type: "text", text: `BUILD OK (${arch})\n${log.slice(-3000)}` }],
				details: { arch, oDir },
			};
		},
	});

	pi.registerTool({
		name: "kunit_run",
		label: "KUnit",
		description:
			"Run KUnit tests (needs QEMU; degrades gracefully). Layer extra kunitconfigs on top of rust/.kunitconfig.",
		promptSnippet: "Run in-kernel unit tests via KUnit",
		parameters: Type.Object({
			kunitconfigs: Type.Optional(
				Type.Array(Type.String(), {
					description: "Config fragments, e.g. ['rust/.kunitconfig','rewrite/testing/kunit/rust-extended.kunitconfig']",
				}),
			),
			filter: Type.Optional(Type.String({ description: "Test name glob filter" })),
		}),
		async execute(_id, p, signal, _onUpdate, ctx) {
			const root = ctx.cwd ?? process.cwd();
			const frags = p.kunitconfigs ?? ["rust/.kunitconfig"];
			const args = frags.flatMap((f) => ["--kunitconfig", f]);
			if (p.filter) args.push("--filter", p.filter);
			const script = `cd "${root}" && python3 tools/testing/kunit/kunit.py run ${args.map((a) => `'${a}'`).join(" ")}`;
			try {
				const out = await sh(root, script, BUILD_TIMEOUT_MS, signal);
				return { content: [{ type: "text", text: out.slice(-5000) }], details: {} };
			} catch (e) {
				const msg = String(e);
				if (/qemu|kvmtool|No such file/i.test(msg)) {
					return {
						content: [
							{
								type: "text",
								text: "SKIP: QEMU unavailable in this environment. Record this gap per PROTOCOLS.md §5 honesty rules and request runner-host verification.",
							},
						],
						details: {},
					};
				}
				throw e;
			}
		},
	});

	pi.registerTool({
		name: "safety_audit",
		label: "Safety Audit",
		description:
			"Audit unsafe blocks for // SAFETY: comments across the Rust tree. Fails on new violations vs baseline.",
		promptSnippet: "unsafe-block SAFETY-comment audit",
		parameters: Type.Object({
			report_only: Type.Optional(Type.Boolean()),
		}),
		async execute(_id, p, signal, _onUpdate, ctx) {
			const root = ctx.cwd ?? process.cwd();
			const mode = p.report_only ? "--report-only" : "--baseline rewrite/ci/unsafe-baseline.txt";
			try {
				const out = await sh(root, `python3 rewrite/ci/safety-audit.py ${mode}`, SHORT_TIMEOUT_MS, signal);
				return { content: [{ type: "text", text: out.slice(-4000) }], details: {} };
			} catch (e) {
				throw new Error(`SAFETY AUDIT FAILED — new uncommented unsafe found:\n${String(e).slice(-3500)}`);
			}
		},
	});

	pi.registerTool({
		name: "get_maintainer",
		label: "Get Maintainers",
		description:
			"Resolve reviewers/mailing lists for a file or patch via scripts/get_maintainer.pl. Use before submitting any series.",
		promptSnippet: "MAINTAINERS lookup for patches",
		parameters: Type.Object({
			path: Type.String({ description: "File or patch path (strip leading @)" }),
		}),
		async execute(_id, p, signal, _onUpdate, ctx) {
			const root = ctx.cwd ?? process.cwd();
			const target = resolve(root, p.path.replace(/^@/, ""));
			const out = await sh(
				root,
				`./scripts/get_maintainer.pl --no-rolestats '${target}'`,
				SHORT_TIMEOUT_MS,
				signal,
			);
			return { content: [{ type: "text", text: out }], details: {} };
		},
	});

	pi.registerTool({
		name: "tracker_update",
		label: "Tracker Update",
		description:
			"Update a row in rewrite/TRACKER.md (status/owner/notes). Team-wide state changes MUST go through this tool.",
		promptSnippet: "Update rewrite/TRACKER.md status rows",
		promptGuidelines: [
			"Use tracker_update whenever port status changes instead of editing TRACKER.md by hand, to keep rows well-formed.",
		],
		parameters: Type.Object({
			row_id: Type.String({ description: "e.g. P-001" }),
			status: Type.Optional(
				Type.String({ description: "backlog|spec|porting|testing|review|ready|submitted|merged|blocked|abandoned" }),
			),
			owner: Type.Optional(Type.String()),
			layers: Type.Optional(Type.String({ description: "layer codes done, e.g. 'unit,prop'" })),
			append_notes: Type.Optional(Type.String({ description: "text appended to Notes cell" })),
		}),
		async execute(_id, p, _signal, _onUpdate, ctx) {
			const root = ctx.cwd ?? process.cwd();
			const trackerPath = resolve(root, "rewrite/TRACKER.md");

			return withFileMutationQueue(trackerPath, async () => {
				const { readFile, writeFile } = await import("node:fs/promises");
				const text = await readFile(trackerPath, "utf8");
				const lines = text.split("\n");
				let hit = false;
				for (let i = 0; i < lines.length; i++) {
					if (!lines[i].startsWith(`| ${p.row_id} `)) continue;
					hit = true;
					const cells = lines[i].split("|").map((c) => c.trim());
					// | ID | Target | Phase | Owner | Status | Layers | unsafe% | Notes |
					if (p.owner) cells[4] = p.owner;
					if (p.status) cells[5] = p.status;
					if (p.layers) cells[6] = p.layers;
					if (p.append_notes) cells[8] = cells[8] && cells[8] !== "Notes"
						? `${cells[8]}; ${p.append_notes}`
						: p.append_notes;
					lines[i] = `| ${cells.slice(1, 9).join(" | ")} |`;
					break;
				}
				if (!hit) {
					fail(`Row ${p.row_id} not found in rewrite/TRACKER.md. Add it manually with all columns.`);
				}
				await writeFile(trackerPath, lines.join("\n"));
				return {
					content: [{ type: "text", text: `TRACKER updated: ${p.row_id}` }],
					details: {},
				};
			});
		},
	});

	void dirname;
	void join;
}
