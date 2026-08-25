/**
 * Self-hosted Monaco wiring: bundled locally instead of the default CDN
 * loader so the desktop app works fully offline. Imported lazily right
 * before the editor chunk loads (keeps monaco out of cold-start & tests).
 */
import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";

loader.config({ monaco });
