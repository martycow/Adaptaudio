/*
 * Adaptaudio -> FMOD Studio importer.
 *
 * Reads an Adaptaudio export folder (manifest.json + stems/*.wav) and builds the music event:
 * parameters, audio tracks with looping stems, loop regions, destination markers, transition
 * regions on the labeled "Mode" parameter, volume/effect automation and one-shot stinger events.
 *
 * Install: put this file in the "Scripts" folder of your FMOD Studio project (or the user scripts
 * folder), then Scripts > Reload. Run: Scripts > Adaptaudio > Import scene...
 *
 * HONESTY NOTE: this script was written without access to FMOD Studio. Every API call the author
 * is not certain of is marked "VERIFY:". Each risky step is wrapped so that a failure does not stop
 * the import: it is logged as a MANUAL step instead. At the end the script prints the list of
 * manual steps to the Console (Window > Console) and writes it to <export>/import_log.txt.
 *
 * The script engine is plain ES5: no let/const, no arrow functions, no template strings.
 */

var AA = {
    MANIFEST_VERSION: 1,
    log: [],
    manual: [],
    audioFiles: {},
    crossfades: {}
};

function aaLog(message) {
    AA.log.push(message);
    console.log("[Adaptaudio] " + message);
}

function aaManual(message) {
    AA.manual.push(message);
    console.log("[Adaptaudio] MANUAL: " + message);
}

/* Runs fn; on any exception logs a manual step and returns null instead of stopping the import. */
function aaTry(what, fn) {
    try {
        return fn();
    } catch (e) {
        aaManual(what + " (script failed: " + e + ")");
        return null;
    }
}

/* Sets obj[prop] only if the property exists, so a wrong property name is reported, not ignored. */
function aaSet(obj, prop, value, what) {
    try {
        if (obj === null || obj === undefined || !(prop in obj)) {
            throw new Error("no property '" + prop + "'");
        }
        obj[prop] = value;
        return true;
    } catch (e) {
        aaManual(what + ": set " + prop + " = " + value + " by hand (" + e + ")");
        return false;
    }
}

function aaJoin(folder, file) {
    var f = String(folder).replace(/\\/g, "/");
    if (f.charAt(f.length - 1) !== "/") {
        f += "/";
    }
    return f + file;
}

function aaReadText(path) {
    // VERIFY: studio.system.getFile(path), File.exists(), File.open(studio.system.openMode.ReadOnly),
    // File.size() and File.readText(size).
    var file = studio.system.getFile(path);
    if (!file.exists()) {
        throw new Error("file not found: " + path);
    }
    file.open(studio.system.openMode.ReadOnly);
    var text = file.readText(file.size());
    file.close();
    return text;
}

function aaWriteText(path, text) {
    // VERIFY: studio.system.openMode.WriteOnly and File.writeText(text).
    var file = studio.system.getFile(path);
    file.open(studio.system.openMode.WriteOnly);
    file.writeText(text);
    file.close();
}

/* Asks for the export folder: a folder picker dialog, falling back to a plain text prompt. */
function aaAskFolder(onFolder) {
    try {
        // VERIFY: studio.ui.showModalDialog layout, widgetType.PathLineEdit, pathType.Directory,
        // findWidget(id).text() and closeDialog().
        studio.ui.showModalDialog({
            windowTitle: "Adaptaudio: import scene",
            windowWidth: 560,
            widgetType: studio.ui.widgetType.Layout,
            layout: studio.ui.layoutType.VBoxLayout,
            items: [
                {
                    widgetType: studio.ui.widgetType.Label,
                    text: "Export folder (the one that contains manifest.json and stems/):"
                },
                {
                    widgetType: studio.ui.widgetType.PathLineEdit,
                    widgetId: "aa_folder",
                    text: "",
                    pathType: studio.ui.pathType.Directory
                },
                {
                    widgetType: studio.ui.widgetType.PushButton,
                    text: "Import",
                    onClicked: function () {
                        var folder = this.findWidget("aa_folder").text();
                        this.closeDialog();
                        onFolder(folder);
                    }
                }
            ]
        });
        return;
    } catch (e) {
        aaLog("Folder dialog is unavailable (" + e + "), asking for a typed path instead.");
    }
    // VERIFY: studio.system.getText(prompt, defaultText) returns the typed string (or null on cancel).
    var typed = studio.system.getText("Adaptaudio export folder (contains manifest.json):", "");
    if (typed) {
        onFolder(typed);
    }
}

/* Finds or creates nested event folders, e.g. ["music"]. */
function aaEnsureFolder(names) {
    // VERIFY: studio.project.workspace.masterEventFolder, EventFolder.items, isOfType("EventFolder"),
    // studio.project.create("EventFolder") and the .folder property that parents it.
    var folder = studio.project.workspace.masterEventFolder;
    for (var i = 0; i < names.length; i++) {
        var found = null;
        var items = folder.items || [];
        for (var j = 0; j < items.length; j++) {
            if (items[j].isOfType("EventFolder") && items[j].name === names[i]) {
                found = items[j];
            }
        }
        if (!found) {
            found = studio.project.create("EventFolder");
            found.name = names[i];
            found.folder = folder;
        }
        folder = found;
    }
    return folder;
}

/* Creates event:/<path>. An existing event with that path is renamed, never deleted. */
function aaCreateEvent(path) {
    var parts = path.split("/");
    var name = parts.pop();
    var folder = aaEnsureFolder(parts);
    // VERIFY: studio.project.lookup("event:/...") returns the event or a falsy value.
    var existing = aaTry("Look up existing event:/" + path, function () {
        return studio.project.lookup("event:/" + path);
    });
    if (existing) {
        var old = name + "_previous";
        var n = 2;
        while (aaTry("Look up event:/" + parts.concat([old]).join("/"), function () {
            return studio.project.lookup("event:/" + parts.concat([old]).join("/"));
        })) {
            old = name + "_previous" + n;
            n += 1;
        }
        existing.name = old;
        aaManual("An event event:/" + path + " already existed. It was renamed to '" + old +
            "'. Compare, then delete it yourself when the new one is right.");
    }
    var event = studio.project.create("Event");
    event.name = name;
    event.folder = folder;
    aaLog("Created event event:/" + path);
    return event;
}

function aaImportAudio(folder, relativeFile) {
    if (AA.audioFiles[relativeFile]) {
        return AA.audioFiles[relativeFile];
    }
    // VERIFY: studio.project.importAudioFile(absolutePath) copies the file into the project's assets
    // and returns an AudioFile (re-importing the same path updates it).
    var audio = studio.project.importAudioFile(aaJoin(folder, relativeFile));
    AA.audioFiles[relativeFile] = audio;
    return audio;
}

/* Places a single instrument with a stem on a track. Positions and lengths are in seconds. */
function aaPlaceStem(event, track, folder, file, start, length, looping) {
    // VERIFY: GroupTrack.addSound(event.timeline, "SingleSound", startSeconds, lengthSeconds),
    // SingleSound.audioFile and SingleSound.looping (the "Loop Instrument" toggle).
    var sound = track.addSound(event.timeline, "SingleSound", start, length);
    sound.audioFile = aaImportAudio(folder, file);
    aaSet(sound, "looping", looping, "Instrument " + file + ": Loop Instrument");
    return sound;
}

/* The object automation curves attach to: FMOD wants the parameter preset, not the proxy. */
function aaPresetOf(parameter) {
    // VERIFY: Event.addGameParameter returns a proxy whose .preset is the ParameterPreset.
    return parameter && parameter.preset ? parameter.preset : parameter;
}

function aaCreateParameter(event, spec) {
    var types = studio.project.parameterType || {};
    // VERIFY: parameter type names. FMOD Studio 2.x uses studio.project.parameterType.User for a
    // continuous user parameter; labeled parameters may be "UserEnumeration".
    var type = spec.kind === "labeled" ? (types.UserEnumeration || types.User) : types.User;
    var param = aaTry("Create parameter '" + spec.name + "' (" + spec.kind + ", " + spec.min + ".." + spec.max + ")",
        function () {
            // VERIFY: Event.addGameParameter({name, type, min, max}).
            return event.addGameParameter({ name: spec.name, type: type, min: spec.min, max: spec.max });
        });
    if (!param) {
        return null;
    }
    var target = param.preset && param.preset.parameter ? param.preset.parameter : param;
    // VERIFY: the parameter property names initialValue, seekSpeed, isDiscrete and enumerationLabels.
    aaSet(target, "initialValue", spec["default"], "Parameter " + spec.name + ": initial value");
    if (spec.seek_speed > 0) {
        aaSet(target, "seekSpeed", spec.seek_speed, "Parameter " + spec.name + ": seek speed (units per second)");
    }
    if (spec.kind === "discrete") {
        aaSet(target, "isDiscrete", true, "Parameter " + spec.name + ": discrete");
    }
    if (spec.kind === "labeled") {
        aaSet(target, "enumerationLabels", spec.labels, "Parameter " + spec.name + ": labels " + spec.labels.join(", "));
    }
    aaLog("Parameter " + spec.name);
    return param;
}

function aaMarkerTrack(event) {
    // VERIFY: Event.markerTracks and Event.addMarkerTrack().
    if (event.markerTracks && event.markerTracks.length > 0) {
        return event.markerTracks[0];
    }
    return event.addMarkerTrack();
}

/* Creates a marker or region object of `type` on the event's marker track. */
function aaCreateOnTimeline(event, type, position, length) {
    // VERIFY: studio.project.create("TempoMarker" | "NamedMarker" | "LoopRegion" | "TransitionRegion"),
    // and that .position/.length are in seconds and .markerTrack (or .timeline) attaches it.
    var item = studio.project.create(type);
    item.position = position;
    if (length !== null && length !== undefined) {
        item.length = length;
    }
    if ("markerTrack" in item) {
        item.markerTrack = aaMarkerTrack(event);
    } else {
        item.timeline = event.timeline;
    }
    return item;
}

function aaAddEffect(track, typeNames, what) {
    // VERIFY: MixerGroup.effectChain.addEffect(typeName) and the effect class names below.
    for (var i = 0; i < typeNames.length; i++) {
        try {
            var effect = track.mixerGroup.effectChain.addEffect(typeNames[i]);
            if (effect) {
                return effect;
            }
        } catch (e) {
            // try the next candidate name
        }
    }
    aaManual(what + ": add a " + typeNames[0] + " to the track by hand");
    return null;
}

/* Adds an automation curve for `property` of `owner` by `parameter`, one point per manifest point. */
function aaAutomate(owner, property, parameter, points, what) {
    return aaTry(what + ": automate '" + property + "' with points " + JSON.stringify(points), function () {
        // VERIFY: ManagedObject.addAutomator(propertyName), Automator.addAutomationCurve(parameterPreset)
        // and AutomationCurve.addAutomationPoint(parameterValue, propertyValue).
        // VERIFY: when two curves automate one property (e.g. Bells: PuzzleProgress and Unlocked_*),
        // FMOD combines them; check the result by ear.
        var automator = owner.addAutomator(property);
        var curve = automator.addAutomationCurve(aaPresetOf(parameter));
        for (var i = 0; i < points.length; i++) {
            curve.addAutomationPoint(points[i][0], points[i][1]);
        }
        return curve;
    });
}

var AA_EFFECTS = {
    // automation target -> [effect class candidates, automated property]
    lowpass_cutoff: [["LowpassEffect", "LowPassEffect"], "cutoff"],
    distortion_level: [["DistortionEffect"], "level"],
    chorus_depth: [["ChorusEffect"], "depth"],
    flanger_mix: [["FlangerEffect"], "mix"]
};

function aaBuildTrack(event, folder, spec, params) {
    // VERIFY: Event.addGroupTrack(name) returns a GroupTrack; MixerGroup.volume is in dB.
    var track = event.addGroupTrack(spec.name);
    if (track.mixerGroup && track.mixerGroup.name !== spec.name) {
        aaSet(track.mixerGroup, "name", spec.name, "Track " + spec.name + ": name");
    }
    aaSet(track.mixerGroup, "volume", spec.volume_db, "Track " + spec.name + ": fader volume (dB)");
    if (spec.pan !== 0) {
        aaTry("Track " + spec.name + ": pan " + Math.round(spec.pan * 100), function () {
            // VERIFY: the track panner object and its property name (stereoPan? in -100..100).
            var panner = track.mixerGroup.panner;
            if (!aaSet(panner, "stereoPan", spec.pan * 100, "Track " + spec.name + ": pan")) {
                return null;
            }
            return panner;
        });
    }

    for (var i = 0; i < spec.instruments.length; i++) {
        var inst = spec.instruments[i];
        aaTry("Track " + spec.name + ": place " + inst.stem + " at " + inst.start_seconds + " s, " +
            inst.length_seconds + " s long, looping", (function (inst) {
                return function () {
                    return aaPlaceStem(event, track, folder, inst.stem, inst.start_seconds, inst.length_seconds, inst.looping);
                };
            })(inst));
    }

    var gain = null;
    for (var a = 0; a < spec.automation.length; a++) {
        var auto = spec.automation[a];
        var what = "Track " + spec.name + ": " + auto.source + " (" + auto.unit + ")";
        var param = params[auto.parameter];
        if (auto.prototype_only) {
            aaLog(what + " is prototype-only; skipped (see density_variants).");
            continue;
        }
        if (!param) {
            aaManual(what + ": parameter " + auto.parameter + " is missing, automate by hand: " + JSON.stringify(auto.points));
            continue;
        }
        if (auto.target === "track_volume") {
            // Volume curves are offsets from the fader, so they go on a Gain effect and the fader stays put.
            if (!gain) {
                gain = aaAddEffect(track, ["GainEffect"], what);
            }
            if (gain) {
                // VERIFY: the Gain effect's property is "gain" (dB).
                aaAutomate(gain, "gain", param, auto.points, what);
            }
        } else if (auto.target === "reverb_send_level") {
            aaManual(what + ": add a send to the reverb return and automate its level by " + auto.parameter +
                ": " + JSON.stringify(auto.points));
        } else if (AA_EFFECTS[auto.target]) {
            var fx = aaAddEffect(track, AA_EFFECTS[auto.target][0], what);
            if (fx) {
                aaAutomate(fx, AA_EFFECTS[auto.target][1], param, auto.points, what);
            }
            if (auto.target === "flanger_mix" && fx) {
                aaSet(fx, "rate", 0.3, what + ": slow flanger rate (Hz)");
            }
        } else {
            aaManual(what + ": unknown target " + auto.target + ", automate by hand: " + JSON.stringify(auto.points));
        }
    }
    aaLog("Track " + spec.name + ": " + spec.instruments.length + " instrument(s)");
    return track;
}

function aaQuantizationNote(q) {
    if (q.interval === "free") {
        return "no quantization";
    }
    return q.phrase_bars ? (q.phrase_bars + "-bar phrase (bar-quantized, last bar of each phrase)") : q.interval;
}

function aaBuildTransition(event, tr, markers, params) {
    var marker = markers[tr.destination_marker];
    var mode = params.Mode;
    var what = "Transition " + tr.from_mode + " -> " + tr.to_mode + " [" + tr.rule + "]";
    for (var s = 0; s < tr.segments.length; s++) {
        var seg = tr.segments[s];
        aaTry(what + ": transition region " + seg.start_seconds + " s +" + seg.length_seconds + " s to marker " +
            tr.destination_marker + ", condition Mode = " + tr.condition.label + ", quantization " +
            aaQuantizationNote(tr.quantization), (function (seg) {
                return function () {
                    var region = aaCreateOnTimeline(event, "TransitionRegion", seg.start_seconds, seg.length_seconds);
                    // VERIFY: TransitionRegion.destination takes the NamedMarker.
                    region.destination = marker;
                    // VERIFY: quantization property names and values on TransitionRegion
                    // (isQuantized / quantizationInterval with "Bar" / "Beat").
                    if (tr.quantization.interval === "free") {
                        aaSet(region, "isQuantized", false, what + ": quantization off");
                    } else {
                        aaSet(region, "isQuantized", true, what + ": quantization on");
                        aaSet(region, "quantizationInterval", tr.quantization.interval === "beat" ? "Beat" : "Bar",
                            what + ": quantization interval");
                    }
                    // VERIFY: addParameterCondition(parameterPreset, min, max); a labeled value is its index.
                    region.addParameterCondition(aaPresetOf(mode), tr.condition.value, tr.condition.value);
                    return region;
                };
            })(seg));
    }
    if (tr.crossfade_ms > 0) {
        // Grouped by length so the manual list stays short (see aaReportCrossfades).
        var key = Math.round(tr.crossfade_ms) + " ms (" + tr.crossfade_beats + " beats)";
        AA.crossfades[key] = (AA.crossfades[key] || []).concat([tr.from_mode + " -> " + tr.to_mode]);
    }
    if (tr.stinger_event) {
        aaManual(what + ": stinger '" + tr.stinger_event + "' — put " + (tr.stinger_stem || "its stem") +
            " on the transition timeline, or let the game start the one-shot event");
    }
}

function aaReportCrossfades() {
    for (var key in AA.crossfades) {
        if (AA.crossfades.hasOwnProperty(key)) {
            // Crossfades live on transition timelines, which the script does not build.
            aaManual("Crossfade " + key + ": open each transition region, add a transition timeline and crossfade " +
                "the source out and the destination in over that length: " + AA.crossfades[key].join(", "));
        }
    }
}

function aaBuildStinger(folder, scene, spec) {
    var event = aaCreateEvent(spec.event_path);
    var track = event.addGroupTrack(spec.name);
    aaTry("Stinger " + spec.name + ": place " + spec.stinger_stem, function () {
        return aaPlaceStem(event, track, folder, spec.stinger_stem, 0, spec.length_seconds, false);
    });
    aaLog("Stinger event event:/" + spec.event_path + " (" + spec.length_seconds + " s), the game starts it " +
        aaQuantizationNote(spec.quantization) + "-quantized");
    return event;
}

function aaImport(folder) {
    AA.log = [];
    AA.manual = [];
    AA.audioFiles = {};
    AA.crossfades = {};
    var manifest;
    try {
        manifest = JSON.parse(aaReadText(aaJoin(folder, "manifest.json")));
    } catch (e) {
        studio.system.message("Adaptaudio: can't read manifest.json in " + folder + "\n" + e);
        return;
    }
    if (manifest.manifest_version !== AA.MANIFEST_VERSION) {
        studio.system.message("Adaptaudio: manifest_version " + manifest.manifest_version + " is not supported (expected " +
            AA.MANIFEST_VERSION + "). Update the script.");
        return;
    }
    aaLog("Importing " + manifest.scene + " (" + manifest.tempo_bpm + " BPM, " + manifest.beats_per_bar + "/4, " +
        manifest.key + " " + manifest.scale + ")");

    var event = aaTry("Create event event:/" + manifest.event_path, function () {
        return aaCreateEvent(manifest.event_path);
    });
    if (!event) {
        aaFinish(folder);
        return;
    }

    var params = {};
    for (var p = 0; p < manifest.parameters.length; p++) {
        params[manifest.parameters[p].name] = aaCreateParameter(event, manifest.parameters[p]);
    }

    var tm = manifest.timing.tempo_marker;
    aaTry("Tempo marker " + tm.tempo_bpm + " BPM " + tm.beats_per_bar + "/" + tm.beat_unit + " at 0 s", function () {
        var marker = aaCreateOnTimeline(event, "TempoMarker", tm.seconds, null);
        // VERIFY: TempoMarker.tempo, .timeSignatureNumerator, .timeSignatureDenominator.
        aaSet(marker, "tempo", tm.tempo_bpm, "Tempo marker: tempo");
        aaSet(marker, "timeSignatureNumerator", tm.beats_per_bar, "Tempo marker: beats per bar");
        aaSet(marker, "timeSignatureDenominator", tm.beat_unit, "Tempo marker: beat unit");
        return marker;
    });

    for (var r = 0; r < manifest.regions.length; r++) {
        var region = manifest.regions[r];
        aaTry("Loop region '" + region.name + "' " + region.start_seconds + " s +" + region.length_seconds + " s",
            (function (region) {
                return function () {
                    var loop = aaCreateOnTimeline(event, "LoopRegion", region.start_seconds, region.length_seconds);
                    aaSet(loop, "name", region.name, "Loop region " + region.name + ": name");
                    return loop;
                };
            })(region));
    }

    var markers = {};
    for (var d = 0; d < manifest.destination_markers.length; d++) {
        var dm = manifest.destination_markers[d];
        markers[dm.name] = aaTry("Destination marker '" + dm.name + "' at " + dm.seconds + " s", (function (dm) {
            return function () {
                var marker = aaCreateOnTimeline(event, "NamedMarker", dm.seconds, null);
                marker.name = dm.name;
                return marker;
            };
        })(dm));
    }

    for (var t = 0; t < manifest.tracks.length; t++) {
        var spec = manifest.tracks[t];
        aaTry("Track " + spec.name, (function (spec) {
            return function () {
                return aaBuildTrack(event, folder, spec, params);
            };
        })(spec));
    }

    if (params.Mode) {
        for (var x = 0; x < manifest.transitions.length; x++) {
            aaBuildTransition(event, manifest.transitions[x], markers, params);
        }
        aaReportCrossfades();
    } else if (manifest.transitions.length > 0) {
        aaManual("The Mode parameter could not be created, so no transition regions were built (" +
            manifest.transitions.length + " in manifest.json).");
    }

    for (var e = 0; e < manifest.events.length; e++) {
        var ev = manifest.events[e];
        if (ev.event_path && ev.stinger_stem) {
            aaTry("Stinger event " + ev.event_path, (function (ev) {
                return function () {
                    return aaBuildStinger(folder, manifest.scene, ev);
                };
            })(ev));
        }
    }

    for (var v = 0; v < manifest.density_variants.length; v++) {
        var dv = manifest.density_variants[v];
        aaManual("Density for " + dv.track + " by " + dv.parameter + " (prototype only): " + dv.suggestion);
    }
    for (var w = 0; w < manifest.warnings.length; w++) {
        aaLog("Export warning: " + manifest.warnings[w]);
    }
    for (var g = 0; g < manifest.game_side.length; g++) {
        aaLog("Game side: " + manifest.game_side[g]);
    }

    aaTry("Save the project (File > Save)", function () {
        // VERIFY: studio.project.save().
        return studio.project.save();
    });
    aaFinish(folder);
}

function aaFinish(folder) {
    var text = "Adaptaudio import log\n\n" + AA.log.join("\n") + "\n\nMANUAL STEPS (" + AA.manual.length + ")\n" +
        (AA.manual.length ? "- " + AA.manual.join("\n- ") : "none") + "\n";
    aaTry("Write " + aaJoin(folder, "import_log.txt"), function () {
        aaWriteText(aaJoin(folder, "import_log.txt"), text);
        return true;
    });
    // VERIFY: studio.system.message(text) shows a message box.
    studio.system.message("Adaptaudio import finished. Manual steps: " + AA.manual.length +
        ".\nSee Window > Console and import_log.txt in the export folder.");
}

// VERIFY: studio.menu.addMenuItem({name, execute}); a backslash in the name makes a submenu.
studio.menu.addMenuItem({
    name: "Adaptaudio\\Import scene...",
    execute: function () {
        aaAskFolder(aaImport);
    }
});
