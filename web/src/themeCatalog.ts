// herdr 2ecf19a src/app/state.rs Palette의 색 자료(Apache-2.0).
// 터미널은 WezTerm 내장 scheme·One Dark·Kanagawa 정본. docs/theme-catalog.md 참조.
export const THEME_CATALOG = [
  {
    "id": "tokyo-night",
    "label": "Tokyo Night",
    "mode": "dark",
    "pair": "tokyo-night-day",
    "license": "tokyo",
    "colors": {
      "accent": "#7aa2f7",
      "panel_bg": "#1a1b26",
      "active_row_bg": "#232636",
      "selection_bg": "#2d3650",
      "surface0": "#24283b",
      "surface1": "#414868",
      "surface_dim": "#1a1b26",
      "overlay0": "#565f89",
      "overlay1": "#697196",
      "text": "#c0caf5",
      "subtext0": "#a9b1d6",
      "mauve": "#bb9af7",
      "green": "#9ece6a",
      "yellow": "#e0af68",
      "red": "#f7768e",
      "blue": "#7aa2f7",
      "teal": "#7dcfff",
      "peach": "#ff9e64"
    },
    "terminal": {
      "ansi": [
        "#15161e",
        "#f7768e",
        "#9ece6a",
        "#e0af68",
        "#7aa2f7",
        "#bb9af7",
        "#7dcfff",
        "#a9b1d6"
      ],
      "brights": [
        "#414868",
        "#f7768e",
        "#9ece6a",
        "#e0af68",
        "#7aa2f7",
        "#bb9af7",
        "#7dcfff",
        "#c0caf5"
      ],
      "background": "#1a1b26",
      "foreground": "#c0caf5",
      "cursor_bg": "#c0caf5",
      "cursor_fg": "#1a1b26",
      "selection_bg": "#283457",
      "selection_fg": "#c0caf5"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [],
      "author": "folke",
      "name": "Tokyo Night",
      "origin_url": "https://github.com/folke/tokyonight.nvim",
      "prefix": "t",
      "wezterm_version": "20230712-072601-f4abf8fd"
    }
  },
  {
    "id": "tokyo-night-day",
    "label": "Tokyo Night Day",
    "mode": "light",
    "pair": "tokyo-night",
    "license": "tokyo",
    "colors": {
      "accent": "#2e7de9",
      "panel_bg": "#e1e2e7",
      "active_row_bg": "#d2d3da",
      "selection_bg": "#b6cae7",
      "surface0": "#c4c8da",
      "surface1": "#a8aecb",
      "surface_dim": "#d2d3da",
      "overlay0": "#8990b3",
      "overlay1": "#68709a",
      "text": "#3760bf",
      "subtext0": "#6172b0",
      "mauve": "#7847bd",
      "green": "#587539",
      "yellow": "#8c6c3e",
      "red": "#f52a65",
      "blue": "#2e7de9",
      "teal": "#118c74",
      "peach": "#b15c00"
    },
    "terminal": {
      "ansi": [
        "#e9e9ed",
        "#f52a65",
        "#587539",
        "#8c6c3e",
        "#2e7de9",
        "#9854f1",
        "#007197",
        "#6172b0"
      ],
      "brights": [
        "#a1a6c5",
        "#f52a65",
        "#587539",
        "#8c6c3e",
        "#2e7de9",
        "#9854f1",
        "#007197",
        "#3760bf"
      ],
      "background": "#e1e2e7",
      "foreground": "#3760bf",
      "cursor_bg": "#3760bf",
      "cursor_fg": "#e1e2e7",
      "selection_bg": "#b6bfe2",
      "selection_fg": "#3760bf"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [],
      "author": "folke",
      "name": "Tokyo Night Day",
      "origin_url": "https://github.com/folke/tokyonight.nvim",
      "prefix": "t",
      "wezterm_version": "20230712-072601-f4abf8fd"
    }
  },
  {
    "id": "one-dark",
    "label": "One Dark",
    "mode": "dark",
    "pair": "one-light",
    "license": "one-dark",
    "colors": {
      "accent": "#61afef",
      "panel_bg": "#282c34",
      "active_row_bg": "#313640",
      "selection_bg": "#334659",
      "surface0": "#2c313a",
      "surface1": "#3e4451",
      "surface_dim": "#282c34",
      "overlay0": "#5c6370",
      "overlay1": "#737a87",
      "text": "#abb2bf",
      "subtext0": "#969ca8",
      "mauve": "#c678dd",
      "green": "#98c379",
      "yellow": "#e5c07b",
      "red": "#e06c75",
      "blue": "#61afef",
      "teal": "#56b6c2",
      "peach": "#d19a66"
    },
    "terminal": {
      "ansi": [
        "#282c34",
        "#e06c75",
        "#98c379",
        "#e5c07b",
        "#61afef",
        "#c678dd",
        "#56b6c2",
        "#abb2bf"
      ],
      "brights": [
        "#3e4452",
        "#be5046",
        "#98c379",
        "#d19a66",
        "#61afef",
        "#c678dd",
        "#56b6c2",
        "#5c6370"
      ],
      "background": "#282c34",
      "foreground": "#abb2bf",
      "cursor_bg": "#61afef",
      "cursor_fg": "#282c34",
      "selection_bg": "#3e4452",
      "selection_fg": "#abb2bf"
    },
    "terminalSource": {
      "name": "onedark.vim",
      "revision": "47bec7a6196a843dad195d2666c3ac84c6e80c78",
      "origin_url": "https://github.com/joshdick/onedark.vim",
      "files": [
        "autoload/onedark.vim",
        "colors/onedark.vim"
      ]
    }
  },
  {
    "id": "one-light",
    "label": "One Light",
    "mode": "light",
    "pair": "one-dark",
    "license": "one-light",
    "colors": {
      "accent": "#4078f2",
      "panel_bg": "#fafafa",
      "active_row_bg": "#d8dbe2",
      "selection_bg": "#cddbf8",
      "surface0": "#f0f0f1",
      "surface1": "#e5e5e6",
      "surface_dim": "#f5f5f6",
      "overlay0": "#a0a1a7",
      "overlay1": "#686b77",
      "text": "#383a42",
      "subtext0": "#686b77",
      "mauve": "#a626a4",
      "green": "#50a14f",
      "yellow": "#c18401",
      "red": "#e45649",
      "blue": "#4078f2",
      "teal": "#0184bc",
      "peach": "#986801"
    },
    "terminal": {
      "ansi": [
        "#000000",
        "#da3e39",
        "#41933e",
        "#855504",
        "#315eee",
        "#930092",
        "#0e6fad",
        "#8e8f96"
      ],
      "brights": [
        "#2a2b32",
        "#da3e39",
        "#41933e",
        "#855504",
        "#315eee",
        "#930092",
        "#0e6fad",
        "#fffefe"
      ],
      "background": "#f8f8f8",
      "foreground": "#2a2b32",
      "cursor_bg": "#2a2b32",
      "cursor_fg": "#f8f8f8",
      "selection_bg": "#cddbf8",
      "selection_fg": "#2a2b32"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [
        "OneLight (Gogh)"
      ],
      "name": "One Light (Gogh)",
      "origin_url": "https://github.com/Gogh-Co/Gogh",
      "prefix": "o",
      "wezterm_version": "20220807-113146-c2fee766"
    }
  },
  {
    "id": "solarized",
    "label": "Solarized Dark",
    "mode": "dark",
    "pair": "solarized-light",
    "license": "solarized",
    "colors": {
      "accent": "#268bd2",
      "panel_bg": "#002b36",
      "active_row_bg": "#164b57",
      "selection_bg": "#083e55",
      "surface0": "#073642",
      "surface1": "#586e75",
      "surface_dim": "#002b36",
      "overlay0": "#586e75",
      "overlay1": "#657b83",
      "text": "#93a1a1",
      "subtext0": "#839496",
      "mauve": "#d33682",
      "green": "#859900",
      "yellow": "#b58900",
      "red": "#dc322f",
      "blue": "#268bd2",
      "teal": "#2aa198",
      "peach": "#cb4b16"
    },
    "terminal": {
      "ansi": [
        "#073642",
        "#dc322f",
        "#859900",
        "#b58900",
        "#268bd2",
        "#d33682",
        "#2aa198",
        "#eee8d5"
      ],
      "brights": [
        "#002b36",
        "#cb4b16",
        "#586e75",
        "#657b83",
        "#839496",
        "#6c71c4",
        "#93a1a1",
        "#fdf6e3"
      ],
      "background": "#002b36",
      "foreground": "#839496",
      "cursor_bg": "#839496",
      "cursor_fg": "#073642",
      "selection_bg": "#073642",
      "selection_fg": "#93a1a1"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [
        "iTerm2 Solarized Dark"
      ],
      "name": "Builtin Solarized Dark",
      "origin_url": "https://github.com/mbadolato/iTerm2-Color-Schemes",
      "prefix": "b",
      "wezterm_version": "Always"
    }
  },
  {
    "id": "solarized-light",
    "label": "Solarized Light",
    "mode": "light",
    "pair": "solarized",
    "license": "solarized",
    "colors": {
      "accent": "#268bd2",
      "panel_bg": "#fdf6e3",
      "active_row_bg": "#eee8d5",
      "selection_bg": "#c9dcdf",
      "surface0": "#eee8d5",
      "surface1": "#93a1a1",
      "surface_dim": "#eee8d5",
      "overlay0": "#93a1a1",
      "overlay1": "#586e75",
      "text": "#657b83",
      "subtext0": "#839496",
      "mauve": "#d33682",
      "green": "#859900",
      "yellow": "#b58900",
      "red": "#dc322f",
      "blue": "#268bd2",
      "teal": "#2aa198",
      "peach": "#cb4b16"
    },
    "terminal": {
      "ansi": [
        "#073642",
        "#dc322f",
        "#859900",
        "#b58900",
        "#268bd2",
        "#d33682",
        "#2aa198",
        "#eee8d5"
      ],
      "brights": [
        "#002b36",
        "#cb4b16",
        "#586e75",
        "#657b83",
        "#839496",
        "#6c71c4",
        "#93a1a1",
        "#fdf6e3"
      ],
      "background": "#fdf6e3",
      "foreground": "#657b83",
      "cursor_bg": "#657b83",
      "cursor_fg": "#eee8d5",
      "selection_bg": "#eee8d5",
      "selection_fg": "#586e75"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [
        "Solarized Light (Gogh)",
        "SolarizedLight (Gogh)",
        "iTerm2 Solarized Light"
      ],
      "name": "Builtin Solarized Light",
      "origin_url": "https://github.com/mbadolato/iTerm2-Color-Schemes",
      "prefix": "b",
      "wezterm_version": "Always"
    }
  },
  {
    "id": "gruvbox",
    "label": "Gruvbox Dark",
    "mode": "dark",
    "pair": "gruvbox-light",
    "license": "gruvbox",
    "colors": {
      "accent": "#d79921",
      "panel_bg": "#282828",
      "active_row_bg": "#323130",
      "selection_bg": "#4b3f27",
      "surface0": "#3c3836",
      "surface1": "#504945",
      "surface_dim": "#282828",
      "overlay0": "#928374",
      "overlay1": "#a89984",
      "text": "#ebdbb2",
      "subtext0": "#d5c4a1",
      "mauve": "#d3869b",
      "green": "#b8bb26",
      "yellow": "#fabd2f",
      "red": "#fb4934",
      "blue": "#83a598",
      "teal": "#8ec07c",
      "peach": "#fe8019"
    },
    "terminal": {
      "ansi": [
        "#282828",
        "#cc241d",
        "#98971a",
        "#d79921",
        "#458588",
        "#b16286",
        "#689d6a",
        "#a89984"
      ],
      "brights": [
        "#928374",
        "#fb4934",
        "#b8bb26",
        "#fabd2f",
        "#83a598",
        "#d3869b",
        "#8ec07c",
        "#ebdbb2"
      ],
      "background": "#282828",
      "foreground": "#ebdbb2",
      "cursor_bg": "#ebdbb2",
      "cursor_fg": "#282828",
      "selection_bg": "#665c54",
      "selection_fg": "#ebdbb2"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [
        "Gruvbox Dark (Gogh)"
      ],
      "name": "GruvboxDark",
      "origin_url": "https://github.com/mbadolato/iTerm2-Color-Schemes",
      "prefix": "g",
      "wezterm_version": "20230320-124340-559cb7b0"
    }
  },
  {
    "id": "gruvbox-light",
    "label": "Gruvbox Light",
    "mode": "light",
    "pair": "gruvbox",
    "license": "gruvbox",
    "colors": {
      "accent": "#076678",
      "panel_bg": "#fbf1c7",
      "active_row_bg": "#f2e5bc",
      "selection_bg": "#ebdbb2",
      "surface0": "#ebdbb2",
      "surface1": "#d5c4a1",
      "surface_dim": "#f2e5bc",
      "overlay0": "#928374",
      "overlay1": "#7c6f64",
      "text": "#3c3836",
      "subtext0": "#504945",
      "mauve": "#8f3f71",
      "green": "#79740e",
      "yellow": "#b57614",
      "red": "#9d0006",
      "blue": "#076678",
      "teal": "#427b58",
      "peach": "#af3a03"
    },
    "terminal": {
      "ansi": [
        "#fbf1c7",
        "#9d0006",
        "#79740e",
        "#b57614",
        "#076678",
        "#8f3f71",
        "#427b58",
        "#3c3836"
      ],
      "brights": [
        "#9d8374",
        "#cc241d",
        "#98971a",
        "#d79921",
        "#458588",
        "#b16186",
        "#689d69",
        "#7c6f64"
      ],
      "background": "#fbf1c7",
      "foreground": "#282828",
      "cursor_bg": "#282828",
      "cursor_fg": "#fbf1c7",
      "selection_bg": "#d5c4a1",
      "selection_fg": "#665c54"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [
        "Gruvbox Light"
      ],
      "name": "GruvboxLight",
      "origin_url": "https://github.com/mbadolato/iTerm2-Color-Schemes",
      "prefix": "g",
      "wezterm_version": "Always"
    }
  },
  {
    "id": "kanagawa",
    "label": "Kanagawa Wave",
    "mode": "dark",
    "pair": "kanagawa-lotus",
    "license": "kanagawa",
    "colors": {
      "accent": "#7e9cd8",
      "panel_bg": "#1f1f28",
      "active_row_bg": "#363646",
      "selection_bg": "#32384b",
      "surface0": "#2a2a37",
      "surface1": "#363646",
      "surface_dim": "#1f1f28",
      "overlay0": "#727169",
      "overlay1": "#87867d",
      "text": "#dcd7ba",
      "subtext0": "#c8c3aa",
      "mauve": "#957fb8",
      "green": "#76946a",
      "yellow": "#c0a36e",
      "red": "#c34043",
      "blue": "#7e9cd8",
      "teal": "#7fb4ca",
      "peach": "#ffa066"
    },
    "terminal": {
      "ansi": [
        "#090618",
        "#c34043",
        "#76946a",
        "#c0a36e",
        "#7e9cd8",
        "#957fb8",
        "#6a9589",
        "#dcd7ba"
      ],
      "brights": [
        "#727169",
        "#e82424",
        "#98bb6c",
        "#e6c384",
        "#7fb4ca",
        "#938aa9",
        "#7aa89f",
        "#c8c093"
      ],
      "background": "#1f1f28",
      "foreground": "#dcd7ba",
      "cursor_bg": "#dcd7ba",
      "cursor_fg": "#1f1f28",
      "selection_bg": "#32384b",
      "selection_fg": "#dcd7ba"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [
        "kanagawa (Gogh)"
      ],
      "name": "Kanagawa (Gogh)",
      "origin_url": "https://github.com/Gogh-Co/Gogh",
      "prefix": "k",
      "wezterm_version": "20230320-124340-559cb7b0"
    }
  },
  {
    "id": "kanagawa-lotus",
    "label": "Kanagawa Lotus",
    "mode": "light",
    "pair": "kanagawa",
    "license": "kanagawa",
    "colors": {
      "accent": "#4d699b",
      "panel_bg": "#f2ecbc",
      "active_row_bg": "#d5cea3",
      "selection_bg": "#dcd5ac",
      "surface0": "#dcd5ac",
      "surface1": "#c9cbd1",
      "surface_dim": "#d5cea3",
      "overlay0": "#a09cac",
      "overlay1": "#8a8980",
      "text": "#545464",
      "subtext0": "#43436c",
      "mauve": "#624c83",
      "green": "#6f894e",
      "yellow": "#77713f",
      "red": "#c84053",
      "blue": "#4d699b",
      "teal": "#4e8ca2",
      "peach": "#cc6d00"
    },
    "terminal": {
      "ansi": [
        "#1f1f28",
        "#c84053",
        "#6f894e",
        "#77713f",
        "#4d699b",
        "#b35b79",
        "#597b75",
        "#545464"
      ],
      "brights": [
        "#8a8980",
        "#d7474b",
        "#6e915f",
        "#836f4a",
        "#6693bf",
        "#624c83",
        "#5e857a",
        "#43436c"
      ],
      "background": "#f2ecbc",
      "foreground": "#545464",
      "cursor_bg": "#545464",
      "cursor_fg": "#f2ecbc",
      "selection_bg": "#c9cbd1",
      "selection_fg": "#dcd7ba"
    },
    "terminalSource": {
      "name": "Kanagawa Lotus",
      "origin_url": "https://github.com/rebelot/kanagawa.nvim/blob/master/extras/alacritty/kanagawa_lotus.toml"
    }
  },
  {
    "id": "rose-pine",
    "label": "Rosé Pine",
    "mode": "dark",
    "pair": "rose-pine-dawn",
    "license": "rose-pine",
    "colors": {
      "accent": "#c4a7e7",
      "panel_bg": "#191724",
      "active_row_bg": "#26233a",
      "selection_bg": "#3b344b",
      "surface0": "#1f1d2e",
      "surface1": "#26233a",
      "surface_dim": "#26233a",
      "overlay0": "#6e6a86",
      "overlay1": "#908caa",
      "text": "#e0def4",
      "subtext0": "#c8c5dc",
      "mauve": "#c4a7e7",
      "green": "#31748f",
      "yellow": "#f6c177",
      "red": "#eb6f92",
      "blue": "#31748f",
      "teal": "#9ccfd8",
      "peach": "#ea9a97"
    },
    "terminal": {
      "ansi": [
        "#26233a",
        "#eb6f92",
        "#9ccfd8",
        "#f6c177",
        "#31748f",
        "#c4a7e7",
        "#ebbcba",
        "#e0def4"
      ],
      "brights": [
        "#6e6a86",
        "#eb6f92",
        "#9ccfd8",
        "#f6c177",
        "#31748f",
        "#c4a7e7",
        "#ebbcba",
        "#e0def4"
      ],
      "background": "#191724",
      "foreground": "#e0def4",
      "cursor_bg": "#e0def4",
      "cursor_fg": "#191724",
      "selection_bg": "#3b344b",
      "selection_fg": "#e0def4"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [],
      "name": "Rosé Pine (Gogh)",
      "origin_url": "https://github.com/Gogh-Co/Gogh",
      "prefix": "r",
      "wezterm_version": "20230712-072601-f4abf8fd"
    }
  },
  {
    "id": "rose-pine-dawn",
    "label": "Rosé Pine Dawn",
    "mode": "light",
    "pair": "rose-pine",
    "license": "rose-pine",
    "colors": {
      "accent": "#907aa9",
      "panel_bg": "#faf4ed",
      "active_row_bg": "#e3d9cf",
      "selection_bg": "#f2e9e1",
      "surface0": "#f2e9e1",
      "surface1": "#fffaf3",
      "surface_dim": "#f2e9e1",
      "overlay0": "#9893a5",
      "overlay1": "#797593",
      "text": "#464261",
      "subtext0": "#797593",
      "mauve": "#907aa9",
      "green": "#286983",
      "yellow": "#ea9d34",
      "red": "#b4637a",
      "blue": "#286983",
      "teal": "#56949f",
      "peach": "#d7827e"
    },
    "terminal": {
      "ansi": [
        "#f2e9e1",
        "#b4637a",
        "#56949f",
        "#ea9d34",
        "#286983",
        "#907aa9",
        "#d7827e",
        "#575279"
      ],
      "brights": [
        "#9893a5",
        "#b4637a",
        "#56949f",
        "#ea9d34",
        "#286983",
        "#907aa9",
        "#d7827e",
        "#575279"
      ],
      "background": "#faf4ed",
      "foreground": "#575279",
      "cursor_bg": "#575279",
      "cursor_fg": "#faf4ed",
      "selection_bg": "#f2e9e1",
      "selection_fg": "#575279"
    },
    "terminalSource": {
      "revision": "e2a98e1b8af156988ad0ac778658ac27b60a55fb",
      "aliases": [],
      "name": "Rosé Pine Dawn (Gogh)",
      "origin_url": "https://github.com/Gogh-Co/Gogh",
      "prefix": "r",
      "wezterm_version": "20230712-072601-f4abf8fd"
    }
  }
] as const;

export type CatalogPalette = typeof THEME_CATALOG[number]["id"];
