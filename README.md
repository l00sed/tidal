# Tidal Cycles

Tidal Cycles (or Tidal for short) is a language for live coding patterns, built on top of [Haskell](https://en.wikipedia.org/wiki/Haskell). It is used to create music by writing code in real-time, allowing for improvisation and experimentation.

I'm running this software on macOS with an M3 ARM chip. To produce a similar setup, you will need to install SuperCollider:

```shell
brew install --cask supercollider
```

This should also install the SuperCollider 3 plugins/extensions directory needed for Tidal's default samples.

To install Tidal Cycles, follow the directions on the [Tidal Cycles website](https://tidalcycles.org/docs/getting-started/macos_install). First, install or update xcode:

```shell
/usr/bin/xcode-select --install
```

Then, run the Tidal bootstrap script. This will install a Haskell toolchain, the [cabal](https://www.haskell.org/cabal/) package manager, the [Pulsar](https://pulsar-edit.dev/) text editor, SuperCollider (with SuperDirt samples library and sc-3 plugins), along with Tidal Cycles itself. If any of the dependencies are already installed, the script will skip them.

```shell
curl https://raw.githubusercontent.com/tidalcycles/tidal-bootstrap/master/tidal-bootstrap.command -sSf | sh
```

After installation, ensure the `sclang`, `ghci`, and `cabal` commands are available in a terminal shell. You may need to edit your `$PATH` environment variable to include these binary directories.

For example, here's what I added to my `~/.zshrc` file:

```bash
# GHCup - The Haskell Toolchain Installer
[ -s "$HOME/.ghcup/env" ] && source ${HOME}/.ghcup/env
# SuperCollider language to PATH
[ -s "/Applications/SuperCollider.app/Contents/MacOS/sclang" ] && export PATH="/Applications/SuperCollider.app/Contents/MacOS:$PATH"
```

- [Tidal Cycles cheatsheet](https://github.com/jokroese/tidal-cheat-sheet/blob/master/tidal-cheat-sheet.pdf)

## Neovim

I'm using Neovim to edit `.tidal` code files with the [`grddavies/tidal.nvim`](https://github.com/grddavies/tidal.nvim) plugin which provides shortcuts for interacting with [Tidal Cycles](https://tidalcycles.org/) and [SuperCollider](https://supercollider.github.io/).

Running the `:TidalLaunch` command starts a Tidal Cycles REPL and a SuperCollider server. The `:TidalQuit` command stops both the REPL and the server. You can also send blocks of Tidal code to the REPL with shortcuts like `<leader><CR>`.

## Treesitter

My configuration includes the Treesitter dependency for syntax highlighting in Haskell and SuperCollider scripts:

```lua
-- Tidal Cycles
{
  "grddavies/tidal.nvim",
  opts = require('plugins.tidal'),
  -- Recommended: Install TreeSitter parsers for Haskell and SuperCollider
  dependencies = {
    "nvim-treesitter/nvim-treesitter",
    opts = {
      ensure_installed = {
        "haskell",
        "supercollider"
      }
    }
  }
}
```

Here's my configuration (mostly defaults):

```lua
local opts = {
  --- Configure TidalLaunch command
  boot = {
    tidal = {
      --- Command to launch ghci with tidal installation
      cmd = "ghci",
      args = {
        "-v0",
      },
      --- Tidal boot file path
      file = vim.api.nvim_get_runtime_file("bootfiles/BootTidal.hs", false)[1],
      enabled = true,
    },
    sclang = {
      --- Command to launch SuperCollider
      cmd = "sclang",
      args = {},
      --- SuperCollider boot file
      file = vim.api.nvim_get_runtime_file("bootfiles/BootSuperDirt.scd", false)[1],
      enabled = true,
    },
    split = "v",
  },
  --- Default keymaps
  --- Set to false to disable all default mappings
  --- @type table | nil
  mappings = {
    send_line = { mode = { "i", "n" }, key = "<S-CR>" },
    send_visual = { mode = { "x" }, key = "<leader><CR>" },
    send_block = { mode = { "i", "n", "x" }, key = "<M-CR>" },
    send_node = { mode = "n", key = "<leader>kk" },
    send_silence = { mode = "n", key = "<leader>d" },
    send_hush = { mode = "n", key = "<leader><Esc>" },
  },
  ---- Configure highlight applied to selections sent to tidal interpreter
  selection_highlight = {
    --- Highlight definition table
    --- see ':h nvim_set_hl' for details
    --- @type vim.api.keyset.highlight
    highlight = { link = "IncSearch" },
    --- Duration to apply the highlight for
    timeout = 150,
  },
}

return opts
```

It will automatically read the Tidal and SuperCollider boot files from the plugin's `bootfiles` directory. You can customize the paths if needed. This is also where the SuperDirt samples library is loaded.
