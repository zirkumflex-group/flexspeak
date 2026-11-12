"""Builds the 500 ml can in Blender from the profile the app's 3D shader uses, and renders it from
the app's camera, to compare the two.

    blender -b -P scripts/can-blender.py -- out.png [out.blend]
"""
import math
import pathlib
import re
import sys

import bpy

args = sys.argv[sys.argv.index("--") + 1:]
shader = pathlib.Path(__file__).parent.parent / "src/ui/can3d.wgsl"
table = re.search(r"PROFILE:.*?= array<vec2<f32>, \d+>\((.*?)\n\);", shader.read_text(), re.S)
profile = [(float(r), float(y)) for r, y in re.findall(r"vec2<f32>\(([\d.]+), ([\d.]+)\)", table[1])]

SEGMENTS = 192
verts, faces = [], []
for step in range(SEGMENTS):
    angle = math.tau * step / SEGMENTS
    for radius, height in profile:
        verts.append((radius * math.cos(angle), radius * math.sin(angle), height))
for step in range(SEGMENTS):
    next_step = (step + 1) % SEGMENTS
    for point in range(len(profile) - 1):
        faces.append((
            step * len(profile) + point,
            next_step * len(profile) + point,
            next_step * len(profile) + point + 1,
            step * len(profile) + point + 1,
        ))

bpy.ops.wm.read_factory_settings(use_empty=True)
mesh = bpy.data.meshes.new("Can500ml")
mesh.from_pydata(verts, [], faces)
mesh.validate()
can = bpy.data.objects.new("Can500ml", mesh)
bpy.context.collection.objects.link(can)
material = bpy.data.materials.new("Aluminium")
material.use_nodes = True
principled = material.node_tree.nodes["Principled BSDF"]
principled.inputs["Base Color"].default_value = (0.8, 0.82, 0.86, 1)
principled.inputs["Metallic"].default_value = 1.0
principled.inputs["Roughness"].default_value = 0.3
mesh.materials.append(material)

# The shader's camera: 620 mm away, tilted down 0.2 radians, the can filling 93% of the height.
PITCH, DISTANCE, FILL, HEIGHT = 0.2, 620.0, 0.93, 168.0
camera_data = bpy.data.cameras.new("Camera")
camera_data.sensor_fit = "VERTICAL"
camera_data.sensor_height = 24.0
camera_data.clip_end = 5000.0
camera_data.angle_y = 2 * math.atan((HEIGHT / FILL / 2) / DISTANCE)
camera = bpy.data.objects.new("Camera", camera_data)
camera.location = (0, -DISTANCE * math.cos(PITCH), HEIGHT / 2 + DISTANCE * math.sin(PITCH))
camera.rotation_euler = (math.pi / 2 - PITCH, 0, 0)
bpy.context.collection.objects.link(camera)
scene = bpy.context.scene
scene.camera = camera
sun = bpy.data.objects.new("Key", bpy.data.lights.new("Key", "SUN"))
sun.data.energy = 4.0
sun.rotation_euler = (1.0, 0.0, -0.6)
bpy.context.collection.objects.link(sun)
world = bpy.data.worlds.new("World")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.6, 0.62, 0.68, 1)
scene.world = world
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x, scene.render.resolution_y = 420, 900
scene.render.filepath = str(pathlib.Path(args[0]).resolve())
bpy.ops.render.render(write_still=True)
if len(args) > 1:
    bpy.ops.wm.save_as_mainfile(filepath=str(pathlib.Path(args[1]).resolve()))
