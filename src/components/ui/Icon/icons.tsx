import type { ReactNode, SVGProps } from "react";

/**
 * Hand-drawn line-art icon set (review §5). Each glyph is a stroked 24×24 path
 * with rounded ends and a touch of irregularity so it reads as "sketched"
 * rather than mechanically perfect. `Icon` injects size/stroke/color; these
 * components only declare the geometry. Add new glyphs here and re-export via
 * `iconRegistry` — nothing else needs to change.
 */
type Glyph = (props: SVGProps<SVGSVGElement>) => ReactNode;

// ── navigation / shell ────────────────────────────────────────────────────
const ChatIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5 5.5h13.5v9.5H9.5l-3.6 3.2v-3.2H5z" />
    <path d="M8.5 9.5h7M8.5 12.2h4.5" />
  </svg>
);
const BoardIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M4.5 4.8h15v14.6H4.5z" />
    <path d="M9.6 4.8v14.6M14.7 4.8v14.6" />
    <path d="M6 8h2.6M11 8h2.6M16 8h2" />
  </svg>
);
const TraceIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="6" cy="12" r="2.4" />
    <circle cx="18" cy="6.5" r="2.4" />
    <circle cx="18" cy="17.5" r="2.4" />
    <path d="M8.2 11 15.8 7.2M8.2 13 15.8 16.8" />
  </svg>
);
const GitIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="6.5" cy="6" r="2.2" />
    <circle cx="6.5" cy="18" r="2.2" />
    <circle cx="17.5" cy="11.5" r="2.2" />
    <path d="M6.5 8.2v7.6M8.6 7.2C12 7.4 13 9 13 11.4h4.3" />
  </svg>
);
const SchedulerIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="12" cy="12.5" r="7.2" />
    <path d="M12 8v4.6l3 1.8M9.5 3.5h5" />
  </svg>
);
const ApprovalsIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="12" cy="12" r="7.4" />
    <path d="M8.4 12.2l2.4 2.4 4.8-5" />
  </svg>
);
const SettingsIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M12 4.2v2.4M12 17.4v2.4M4.2 12h2.4M17.4 12h2.4M6.7 6.7l1.7 1.7M15.6 15.6l1.7 1.7M17.3 6.7l-1.7 1.7M8.4 15.6l-1.7 1.7" />
  </svg>
);
const MenuIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M4.5 7h15M4.5 12h15M4.5 17h15" />
  </svg>
);

// ── window controls ──────────────────────────────────────────────────────
const CloseIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5.5 5.4 18.5 18.6M18.5 5.4 5.5 18.6" />
  </svg>
);
const MinimizeIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5.5 15.5h13" />
  </svg>
);
const MaximizeIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5 8v10.5h11M5 8h8.5L19 4.5M19 4.5v6M19 4.5h-6" />
  </svg>
);
const RestoreIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M7 7.5h9.5V17H7zM5.5 5.5h9.5V14" />
  </svg>
);

// ── actions ──────────────────────────────────────────────────────────────
const PlusIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);
const TrashIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5 6.5h14M9.5 6.5V4.8h5v1.7M7 6.5l1 13h8l1-13M10 9.8v7M14 9.8v7" />
  </svg>
);
const PlayIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M7.5 5.2 18.5 12 7.5 18.8z" />
  </svg>
);
const PauseIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M8.5 5v14M15.5 5v14" />
  </svg>
);
const CheckIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5 12.5 9.5 17 19 6.5" />
  </svg>
);
const WarningIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M12 3.5 21 19H3z" />
    <path d="M12 9.5v5M12 17h.01" />
  </svg>
);
const ArrowRightIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M4.5 12h14.5M13.5 6.5 19.5 12l-6 5.5" />
  </svg>
);
const MoreIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="6" cy="12" r="1.4" />
    <circle cx="12" cy="12" r="1.4" />
    <circle cx="18" cy="12" r="1.4" />
  </svg>
);
const SpinnerIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M12 3.5a8.5 8.5 0 1 0 8.4 7.2" />
  </svg>
);
const SearchIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="10.5" cy="10.5" r="6" />
    <path d="M15 15l5 5" />
  </svg>
);
const ChevronDownIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5.5 9.5 12 15.5l6.5-6" />
  </svg>
);
const EyeIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M3 12s3.5-6 9-6 9 6 9 6-3.5 6-9 6-9-6-9-6z" />
    <circle cx="12" cy="12" r="2.4" />
  </svg>
);
const EyeOffIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M4 4.5 20 20M9.8 9.9A2.4 2.4 0 0 0 12 14.4c1 0 1.9-.6 2.3-1.5M6.5 7.2C4 8.9 3 12 3 12s3.5 6 9 6c1.7 0 3.2-.5 4.5-1.3M10.2 5.6A9 9 0 0 1 12 5.4c5.5 0 9 6 9 6a15 15 0 0 1-2.4 3" />
  </svg>
);
const KeyIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="8" cy="8" r="3.4" />
    <path d="M10.4 10.4 19 19M15.5 14.5l2.5-2.5M17.5 16.5l2-2" />
  </svg>
);
const CopyIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M8.5 8.5h9v10h-9zM5.5 15.5h-1V5.5h10v1" />
  </svg>
);
const SendIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M4.5 12 19.5 4.5 14.5 19.5 11 13.5z" />
  </svg>
);
const StopIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M6.5 6.5h11v11h-11z" />
  </svg>
);
const BranchIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="6" cy="6" r="2" />
    <circle cx="6" cy="18" r="2" />
    <circle cx="18" cy="12" r="2" />
    <path d="M6 8v8M8 6h4a4 4 0 0 1 4 4v0" />
  </svg>
);
const ToolIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M14.5 5.5a4 4 0 0 0-5 5l-5 5 2.5 2.5 5-5a4 4 0 0 0 5-5l-2.5 2.5-2.5-2.5z" />
  </svg>
);
const FileIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M6 3.5h7l5 5v12h-12z" />
    <path d="M13 3.5v5h5" />
  </svg>
);
const UsersIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="9" cy="8.5" r="3" />
    <path d="M3.8 19c.6-3 2.8-4.6 5.2-4.6s4.6 1.6 5.2 4.6" />
    <path d="M16 6.2a3 3 0 0 1 0 5.6M16.4 14.8c2 .4 3.6 2 4.2 4.2" />
  </svg>
);
const SparklesIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M12 4.5l1.6 4.5L18 10.6l-4.4 1.6L12 16.7l-1.6-4.5L6 10.6l4.4-1.6z" />
    <path d="M18.5 4v3M20 5.5h-3M5 16.5v2.5M6.2 17.7H4" />
  </svg>
);
const XCircleIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="12" cy="12" r="8" />
    <path d="M8.8 8.8 15.2 15.2M15.2 8.8 8.8 15.2" />
  </svg>
);
const DragHandleIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="9" cy="6" r="1.2" />
    <circle cx="15" cy="6" r="1.2" />
    <circle cx="9" cy="12" r="1.2" />
    <circle cx="15" cy="12" r="1.2" />
    <circle cx="9" cy="18" r="1.2" />
    <circle cx="15" cy="18" r="1.2" />
  </svg>
);
const NoteIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5 4.5h14v11l-4 4.5H5z" />
    <path d="M15 19.5v-4.5h4.5M8 9h8M8 12.5h5" />
  </svg>
);
const InfoIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="12" cy="12" r="8" />
    <path d="M12 11v5M12 8h.01" />
  </svg>
);

// ── theme thumbnails (review §9.3) ───────────────────────────────────────
const PaperIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M5 4.5h14v15H5z" />
    <path d="M8 8.5h8M8 12h8M8 15.5h5" />
  </svg>
);
const GridIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M4.5 4.5h15v15h-15z" />
    <path d="M9 4.5v15M14.5 4.5v15M4.5 9.5h15M4.5 14.5h15" />
  </svg>
);
const ChalkboardIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M4.5 4.8h15v10.4H4.5z" />
    <path d="M9.5 15.2v3.5M14.5 15.2v3.5M12 18.7v-3.5M8 9.5l3 2.5 4-4.5" />
  </svg>
);
const ContrastIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="12" cy="12" r="8" />
    <path d="M12 4v16a8 8 0 0 0 0-16z" fill="currentColor" stroke="none" />
  </svg>
);
const SunIcon: Glyph = (p) => (
  <svg {...p}>
    <circle cx="12" cy="12" r="3.6" />
    <path d="M12 3.5v2.4M12 18.1v2.4M3.5 12h2.4M18.1 12h2.4M6 6l1.7 1.7M16.3 16.3 18 18M18 6l-1.7 1.7M7.7 16.3 6 18" />
  </svg>
);
const MoonIcon: Glyph = (p) => (
  <svg {...p}>
    <path d="M20 13.5A8 8 0 1 1 10.5 4 6.4 6.4 0 0 0 20 13.5z" />
  </svg>
);

export const iconRegistry = {
  chat: ChatIcon,
  board: BoardIcon,
  trace: TraceIcon,
  git: GitIcon,
  scheduler: SchedulerIcon,
  approvals: ApprovalsIcon,
  settings: SettingsIcon,
  menu: MenuIcon,
  close: CloseIcon,
  minimize: MinimizeIcon,
  maximize: MaximizeIcon,
  restore: RestoreIcon,
  plus: PlusIcon,
  trash: TrashIcon,
  play: PlayIcon,
  pause: PauseIcon,
  check: CheckIcon,
  warning: WarningIcon,
  "arrow-right": ArrowRightIcon,
  more: MoreIcon,
  spinner: SpinnerIcon,
  search: SearchIcon,
  "chevron-down": ChevronDownIcon,
  eye: EyeIcon,
  "eye-off": EyeOffIcon,
  key: KeyIcon,
  copy: CopyIcon,
  send: SendIcon,
  stop: StopIcon,
  branch: BranchIcon,
  tool: ToolIcon,
  file: FileIcon,
  users: UsersIcon,
  sparkles: SparklesIcon,
  "x-circle": XCircleIcon,
  "drag-handle": DragHandleIcon,
  note: NoteIcon,
  info: InfoIcon,
  paper: PaperIcon,
  grid: GridIcon,
  chalkboard: ChalkboardIcon,
  contrast: ContrastIcon,
  sun: SunIcon,
  moon: MoonIcon,
} satisfies Record<string, Glyph>;
