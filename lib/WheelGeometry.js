.pragma library

var TAU = 2 * Math.PI
var MAX_CHILD_STEP = Math.PI / 4
var REACH = 28

function wedgeAngle(count, index) {
  if (count <= 0) return 0
  return -Math.PI / 2 + index * (TAU / count)
}

function wedgeAtAngle(count, angle) {
  if (count <= 0) return -1
  var step = TAU / count
  var start = -Math.PI / 2 - step / 2
  var turns = (angle - start) / TAU
  var normalized = turns - Math.floor(turns)
  return Math.min(count - 1, Math.floor(normalized * count))
}

function childStep(count) {
  return count > 0 ? Math.min(MAX_CHILD_STEP, TAU / count) : 0
}

function childAngle(parentAngle, count, index) {
  return parentAngle + (index - (count - 1) / 2) * childStep(count)
}

function childAtAngle(parentAngle, count, angle) {
  if (count <= 0) return -1
  var step = childStep(count)
  var relative = angle - parentAngle
  relative -= TAU * Math.round(relative / TAU)
  var index = Math.floor((relative + count * step / 2) / step)
  return index >= 0 && index < count ? index : -1
}

function pointAt(wheel, x, y) {
  var miss = { ring: "none", index: -1 }
  if (wheel.count <= 0) return miss
  var dx = Number(x) - wheel.x
  var dy = Number(y) - wheel.y
  var distance = Math.sqrt(dx * dx + dy * dy)
  var extent = wheel.subCount > 0 ? wheel.subOuterRadius : wheel.outerCount > 0 ? wheel.childOuterRadius : wheel.outerRadius
  if (distance < wheel.hubRadius || distance > extent + REACH) return miss
  var angle = Math.atan2(dy, dx)
  if (wheel.subCount > 0 && distance > wheel.childOuterRadius) {
    var sub = childAtAngle(wheel.subParentAngle, wheel.subCount, angle)
    if (sub >= 0) return { ring: "sub", index: sub }
  }
  if (wheel.outerCount > 0 && distance > wheel.outerRadius) {
    var child = childAtAngle(wheel.parentAngle, wheel.outerCount, angle)
    if (child >= 0) return { ring: "outer", index: child }
  }
  return { ring: "inner", index: wedgeAtAngle(wheel.count, angle) }
}
