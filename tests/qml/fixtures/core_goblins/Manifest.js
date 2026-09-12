.pragma library

var manifest = {
  "schemaVersion": 1,
  "id": "kurt.goblin-images",
  "name": "Goblin Images",
  "version": "0.1.0",
  "author": "Kurt Buhler",
  "license": "MIT",
  "description": "The Data Goblins illustration library in a FileBlade blade: synced from Google Drive, searchable, browsable by month",
  "kinds": [
    "service",
    "bar-widget"
  ],
  "entryPoints": {
    "service": "Service.qml",
    "barWidget": "GoblinBarWidget.qml"
  },
  "extensions": {
    "data-goblin.fileblade/helper": [
      {
        "id": "catalog",
        "entry": "bin/goblin-imagesctl",
        "read": [
          "list",
          "status"
        ],
        "timeoutMs": 20000
      }
    ],
    "data-goblin.fileblade/blade": [
      {
        "id": "goblin-images",
        "name": "Goblins",
        "glyph": "\udb80\udee9",
        "icon": "assets/goblin.svg",
        "description": "Data Goblins illustrations from the local library, synced from Google Drive",
        "entry": "blades/Module.qml",
        "provider": "Provider.qml",
        "singleton": true,
        "minHeight": 200,
        "hostContract": 2,
        "settings": {
          "defaults": {
            "library": "~/.claude/skills/goblin-images",
            "stackLayers": true
          },
          "schema": [
            {
              "key": "library",
              "type": "path",
              "label": "Library",
              "description": "Folder holding index.json and the images; the goblin-images skill library by default",
              "maxLength": 1024,
              "placeholder": "~/.claude/skills/goblin-images"
            },
            {
              "key": "stackLayers",
              "type": "boolean",
              "label": "Stack layers and copies",
              "description": "Show one image per drawing: the composed scene instead of its layers, the full-size file instead of its copies"
            }
          ]
        }
      }
    ]
  },
  "barWidget": {
    "displayName": "Goblins",
    "description": "The goblin library as a dropdown under a bar icon, hosted by FileBlade",
    "category": "Plugin",
    "allowMultiple": false,
    "defaults": {
      "showNavbarIcon": false,
      "module": "kurt.goblin-images/goblin-images",
      "glyph": "",
      "width": 460,
      "height": 600
    },
    "schema": [
      {
        "key": "showNavbarIcon",
        "type": "boolean",
        "label": "Show navbar icon",
        "description": "Show the optional Goblins dropdown in this bar. The FileBlade extension remains available when hidden."
      },
      {
        "key": "module",
        "type": "string",
        "label": "Module",
        "description": "FileBlade module id to pop out; any module works here",
        "maxLength": 128
      },
      {
        "key": "glyph",
        "type": "string",
        "label": "Glyph",
        "description": "Bar glyph override; empty draws the goblin icon",
        "maxLength": 16
      },
      {
        "key": "width",
        "type": "integer",
        "label": "Width",
        "min": 240,
        "max": 1600,
        "step": 20
      },
      {
        "key": "height",
        "type": "integer",
        "label": "Height",
        "min": 160,
        "max": 1600,
        "step": 20
      }
    ]
  }
}
