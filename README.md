# ilo-toki
Harmony chat client in the terminal!

## Usage
Run it with `cargo run` once you've cloned the repo. If you'd like, you can install the program with `cargo install --path .`.

There are four basic modes (so far): insert, normal, command, and scroll.

Insert mode is the default mode. In this mode, you can type out a message and send it. If you are in normal mode, you can enter insert mode by pressing <key>i</key>

Normal mode is accessible from all modes by pressing <key>Escape</key>. In this mode, you can access all other modes and perform navigation commands on the message box.

Command mode lets you execute commands related to chatting on Harmony. This includes things like quitting the program, joining other guilds, administration stuff, and changing settings. It is accessible from normal mode by pressing <key>:</key>

Scroll mode lets you scroll through messages using your arrow keys. It also lets you perform actions such as editing and deleting messages. This mode is accessible through the <key>s</key> key in normal mode.


## TODO
 - Markdown
 - Attachments
 - Embeds as links
 - Emoji support (ie, managing and sending them, not necessarily viewing them)
 - Reactions
 - Joining guilds
 - Logging in with username/password
 - Registration
 - Administration stuff
 - Theming
 - Mouse support
 - `:tutorial` command for new users

Things that aren't important but would be neat:
 - Pictures in terminal for terminals that support it
 - Video embeds with yt-dlp/youtube-dl
