# blackbird-id3mover

Helper application that takes a directory of music files (including subdirectories) and moves them to an `output` subdirectory of the specified directory, moving all of the files to match this structure based on ID3 tags:

`%album artist%/%album%/%track number% - %track title% [%disc number%].%file extension%`

The disc number segment will be omitted if not present in the ID3 tags.
