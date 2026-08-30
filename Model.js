// Sorakey plugin model: pure helpers for the bar widget and panel. No QML.

// "keyboard/cherrymx-brown-abs" -> "Cherry MX Brown ABS"
function prettyPackName(id) {
  var s = String(id || "")
  var slash = s.lastIndexOf("/")
  if (slash >= 0) s = s.slice(slash + 1)
  s = s.replace(/[-_]+/g, " ")
  s = s.replace(/\b\w/g, function (c) { return c.toUpperCase() })
  return s
}

// A list of soundpack ids -> [{value, label}] for a Dropdown.
function packOptions(ids) {
  var out = []
  if (!Array.isArray(ids)) return out
  for (var i = 0; i < ids.length; i++) {
    var id = String(ids[i] || "")
    if (id === "") continue
    out.push({ value: id, label: prettyPackName(id) })
  }
  return out
}

// Parse a `sorakey ctl status` line. Returns null on any failure so the
// caller can treat it as "no reading" rather than a crash.
function parseStatus(text) {
  try {
    var o = JSON.parse(String(text || "").trim())
    return (o && typeof o === "object") ? o : null
  } catch (e) {
    return null
  }
}

// Parse a `sorakey ctl packs` line. Returns {keyboard:[]}.
function parsePacks(text) {
  var empty = { keyboard: [] }
  try {
    var o = JSON.parse(String(text || "").trim())
    if (!o || typeof o !== "object") return empty
    var kb = Array.isArray(o.keyboard) ? o.keyboard.filter(function(v){ return typeof v==="string" && v.length>0 }) : []
    return { keyboard: kb }
  } catch (e) {
    return empty
  }
}