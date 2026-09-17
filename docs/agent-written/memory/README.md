This file was written by an agent.

# Memory module

The Memory blade lists the instruction and memory files each agent reads
(project and user scope) through `python/agent_memory` (`list`, `apply`) and
opens them for editing. Its columns are file metrics: bytes, characters,
words, estimated tokens and dates.

Nothing is counted for memory. The usage store holds skill, command and MCP
events only; reads of memory files by an agent are not recorded, so the
Memory blade has no Uses column and no heatmap.
