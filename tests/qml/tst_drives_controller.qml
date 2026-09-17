import QtQuick
import QtTest
import "../../controllers"

TestCase {
  name: "DrivesControllerContract"

  property var requests: []
  property var navigations: []

  Item {
    id: fakeService
    property bool backendReady: false
    property var pendingCallback: null

    function backendRequest(name, arguments, generation, callback) {
      requests.push({ name: name, arguments: arguments })
      pendingCallback = callback
      return name + "-request"
    }
    function reply(response) {
      var callback = pendingCallback
      pendingCallback = null
      callback(response)
    }
    function backendSubscribeTopic() { return "" }
    function cancelBackendRequest() { return true }
    function navigateToLocation(path, targetScreen, mode) { navigations.push({ path: path, mode: mode }) }
  }

  DrivesController {
    id: controller
    service: fakeService
  }

  function volume(overrides) {
    var base = {
      name: "Shared",
      device: "sda3",
      source: "/dev/sda3",
      mountpoint: null,
      mounted: false,
      filesystem: "ntfs",
      label: "Shared",
      bus: "ata",
      size: 1600000000000,
      size_label: "1.6 TB",
      used: null,
      available: null,
      removable: false,
      external: false,
      read_only: false,
      image: null,
      tier: "unmounted",
      needs_authorization: true
    }
    for (var key in overrides) base[key] = overrides[key]
    return base
  }

  function init() {
    requests = []
    navigations = []
    fakeService.pendingCallback = null
    controller.showSystemVolumes = false
    controller.busySource = ""
    controller.error = ""
    controller.model.clear()
  }

  function test_rows_carry_drive_icons_and_hide_system_volumes() {
    controller.apply({ actions: true, volumes: [
      volume({}),
      volume({ name: "ASUS", source: "/dev/sdb1", bus: "usb", removable: true, external: true, tier: "external", mounted: true, mountpoint: "/run/media/kurt/ASUS", used: 400, size: 1000, fraction: 0.5 }),
      volume({ name: "Camera", source: "/dev/mmcblk0p1", bus: "mmc", removable: true, external: true, tier: "external" }),
      volume({ name: "fbtest.img", source: "/dev/loop0", image: "/tmp/fbtest.img", external: true, tier: "external" }),
      volume({ name: "/", source: "/dev/mapper/root", mounted: true, mountpoint: "/", tier: "system" })
    ] })
    compare(controller.count, 4)
    compare(controller.volumeCount, 5)
    compare(controller.actionsAvailable, true)
    compare(controller.model.get(0).name, "ASUS")
    compare(controller.model.get(0).volumeGlyph, "󱊞")
    compare(controller.model.get(0).usedFraction, 0.5)
    compare(controller.model.get(1).name, "Camera")
    compare(controller.model.get(1).volumeGlyph, "󰑹")
    compare(controller.model.get(2).name, "fbtest.img")
    compare(controller.model.get(2).volumeGlyph, "󰗮")
    compare(controller.model.get(3).name, "Shared")
    compare(controller.model.get(3).volumeGlyph, "󰋊")
    compare(controller.allModel.get(4).name, "/")
    compare(controller.allModel.get(4).tier, "system")
    compare(controller.tierLabel("unmounted"), "Not mounted")
    compare(controller.tierLabel("external"), "External")

    controller.showSystemVolumes = true
    controller.apply({ actions: true, volumes: [volume({}), volume({ name: "/", source: "/dev/mapper/root", mounted: true, mountpoint: "/", tier: "system" })] })
    compare(controller.count, 2)
  }

  function test_action_follows_mount_state() {
    var unmounted = controller.actionFor(controller.rowFor(volume({})))
    compare(unmounted.command, "mount-volume")
    compare(unmounted.glyph, "󰄠")
    compare(unmounted.tip, "Mount Shared")
    compare(unmounted.actions[0].text, "Mount")
    compare(unmounted.context.map(function(line) { return line.text }), ["asks for authentication"])

    var readOnly = controller.actionFor(controller.rowFor(volume({ name: "Disc", read_only: true, needs_authorization: false })))
    compare(readOnly.tip, "Mount Disc")
    compare(readOnly.context.map(function(line) { return line.text }), ["mounts read-only"])

    var internal = controller.actionFor(controller.rowFor(volume({ mounted: true, mountpoint: "/mnt/shared" })))
    compare(internal.command, "unmount-volume")
    compare(internal.glyph, "󰇪")
    compare(internal.tip, "Unmount Shared")

    var external = controller.actionFor(controller.rowFor(volume({ name: "ASUS", mounted: true, mountpoint: "/run/media/kurt/ASUS", external: true, tier: "external" })))
    compare(external.command, "eject-volume")
    compare(external.glyph, "󰇪")
    compare(external.tip, "Eject ASUS")
  }

  function test_failed_action_surfaces_the_backend_reason() {
    controller.apply({ actions: true, volumes: [volume({})] })
    controller.runAction("/dev/sda3")
    compare(requests.length, 1)
    compare(requests[0].name, "mount-volume")
    compare(requests[0].arguments, ["--source", "/dev/sda3"])
    compare(controller.busySource, "/dev/sda3")

    controller.runAction("/dev/sda3")
    compare(requests.length, 1)

    fakeService.reply({ ok: false, error: "could not mount /dev/sda3: wrong fs type, bad option, bad superblock on /dev/sda3" })
    compare(controller.busySource, "")
    compare(controller.error, "could not mount /dev/sda3: wrong fs type, bad option, bad superblock on /dev/sda3")

    controller.clearError()
    compare(controller.error, "")
  }

  function test_action_on_a_mounted_external_volume_ejects() {
    controller.apply({ actions: true, volumes: [volume({ name: "ASUS", source: "/dev/sdb1", mounted: true, mountpoint: "/run/media/kurt/ASUS", external: true, tier: "external" })] })
    controller.runAction("/dev/sdb1")
    compare(requests[0].name, "eject-volume")
    fakeService.reply({ ok: true })
    compare(controller.error, "")
    compare(controller.busySource, "")
  }

  function test_actions_wait_for_udisks() {
    controller.apply({ actions: false, volumes: [volume({})] })
    controller.runAction("/dev/sda3")
    compare(requests.length, 0)
    compare(controller.busySource, "")
  }

  function test_open_navigates_when_mounted_and_mounts_otherwise() {
    controller.apply({ actions: true, volumes: [
      volume({}),
      volume({ name: "ASUS", source: "/dev/sdb1", mounted: true, mountpoint: "/run/media/kurt/ASUS", external: true, tier: "external" })
    ] })
    controller.openVolume("/dev/sdb1", null)
    compare(navigations.length, 1)
    compare(navigations[0].path, "/run/media/kurt/ASUS")
    compare(navigations[0].mode, "favorite")

    controller.openVolume("/dev/sda3", null)
    compare(requests.length, 1)
    compare(requests[0].name, "mount-volume")
  }
}
