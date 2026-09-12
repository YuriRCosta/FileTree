import QtQuick
import QtTest
import "../../lib/WheelGeometry.js" as Wheel

TestCase {
  name: "WheelSubgeometry"

  function model(count) {
    return { x: 500, y: 500, hubRadius: 26, outerRadius: 90, childOuterRadius: 141, subOuterRadius: 192,
      count: 4, outerCount: 3, parentAngle: -Math.PI / 2, subCount: count, subParentAngle: -Math.PI / 2 }
  }

  function point(wheel, angle, radius) {
    return Wheel.pointAt(wheel, wheel.x + Math.cos(angle) * radius, wheel.y + Math.sin(angle) * radius)
  }

  function test_each_third_ring_wedge_uses_child_geometry() {
    for (var count = 1; count <= 12; count++) {
      var wheel = model(count)
      for (var index = 0; index < count; index++) {
        var angle = Wheel.childAngle(wheel.subParentAngle, count, index)
        var hit = point(wheel, angle, 170)
        compare(hit.ring, "sub")
        compare(hit.index, index)
      }
    }
  }

  function test_inner_and_outer_rings_retain_their_hit_regions() {
    var wheel = model(3)
    compare(point(wheel, -Math.PI / 2, 60).ring, "inner")
    compare(point(wheel, -Math.PI / 2, 115).ring, "outer")
    compare(point(wheel, -Math.PI / 2, 170).ring, "sub")
    compare(point(wheel, -Math.PI / 2, 221).ring, "none")
    compare(point(wheel, -Math.PI / 2, 10).ring, "none")
    wheel.subCount = 0
    compare(point(wheel, -Math.PI / 2, 180).ring, "none")
  }
}
