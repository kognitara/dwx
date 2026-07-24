# dwx 

> Data Walk eXtended

## Vision & Philosophy

`dwx` is a high-performance, keyboard-centric tiling file manager written in Rust.

At its core, `dwx` is built around the concept of **Mushin** (the state of "no-mind" or frictionless flow). The goal is to provide an instantly responsive, distraction-free environment where navigation and file manipulation become second nature, much like a highly customized modal text editor.

To achieve this uncompromised fluidity, `dwx` relies on a custom, lightweight architecture:
* **Flicker-Free Rendering:** Built directly on low-level `crossterm` primitives, deliberately avoiding heavy UI frameworks to maintain maximum control, minimal overhead, and eliminate screen flickering (even over high-latency SSH connections).
* **Asynchronous Core:** Heavy tasks, like deep directory searching, are decoupled and run in the background via an asynchronous event bus. The UI never freezes.
* **Unified Aesthetics:** Consistent visual styling and syntax highlighting (powered by `syntect`) from file navigation to deep previewing.

## Keyboard Shortcuts

`dwx` is designed for fluid, modal navigation.

### Basic Navigation
| Key | Action |
| :--- | :--- |
| `j` / `↓` | Move cursor down |
| `k` / `↑` | Move cursor up |
| `l` / `→` / `Space` | Enter the selected directory |
| `h` / `←` / `Backspace` | Go to parent directory |

### Actions & Files
| Key | Action |
| :--- | :--- |
| `Enter` | Open file in detailed view (Preview/Scroll) |
| `F2` | Rename the file or directory under the cursor |
| `n` then `d` | Create a new directory (Pending create mode) |
| `n` then `f` | Create a new filename (Pending create mode) |
| `n` then `a` | Create a new archive (Pending create mode) |
| `n` then `h` | Create a new hardlink (Pending create mode) |
| `n` then `s` | Create a new symlink (Pending create mode) |
| `F5` | Manually refresh the view |
| `q` | Quit `dwx` |

### Search & Omnibar
The Omnibar appears at the bottom of the screen for text input and commands.

| Key | Action |
| :--- | :--- |
| `/` | Filter current directory content in real-time |
| `?` | Launch a background deep search within the directory |
| `Esc` | Cancel current action or close the Omnibar |
| `Enter` | Validate input (Rename, Search, etc.) |

### Quick Jumps (Go To...)
Press `g` followed by one of the keys below to instantly teleport to a system directory:

| Sequence | Destination |
| :--- | :--- |
| `g` then `h` | Home directory |
| `g` then `x` | Downloads |
| `g` then `d` | Documents |
| `g` then `p` | Pictures |
| `g` then `v` | Videos |
| `g` then `a` | Music (Audio) |
| `g` then `c` | Configuration (`.config`) |
| `g` then `r` | System Root (`/`) |
