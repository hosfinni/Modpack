#!/usr/bin/env python3
"""Write the content pack (content/hz_content): its data files and its pixel art.

The pack is a plain Horizon content mod (`.hmod`: data and assets, no code). Most of it
is one small JSON file per item per folder, so this script holds the items as tables
and writes every file from them. The textures are drawn here too, pixel by pixel, so
the pack has no binary sources: run this again after changing a table and commit what
it writes.

    python3 tools/make_content.py
"""
import json
from pathlib import Path
import shutil
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
PACK = ROOT / 'content' / 'hz_content'
MOD = 'hz_content'

# --------------------------------------------------------------------------- pixels

CLEAR = (0, 0, 0, 0)


def rgb(value, alpha=255):
    return ((value >> 16) & 255, (value >> 8) & 255, value & 255, alpha)


def shade(color, factor):
    r, g, b, a = color
    return (min(255, int(r * factor)), min(255, int(g * factor)), min(255, int(b * factor)), a)


class Canvas:
    def __init__(self, width, height):
        self.width, self.height = width, height
        self.pixels = [[CLEAR] * width for _ in range(height)]

    def put(self, x, y, color):
        if 0 <= x < self.width and 0 <= y < self.height:
            self.pixels[y][x] = color

    def get(self, x, y):
        if 0 <= x < self.width and 0 <= y < self.height:
            return self.pixels[y][x]
        return CLEAR

    def rect(self, x0, y0, x1, y1, color):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                self.put(x, y, color)

    def line(self, x0, y0, x1, y1, color):
        steps = max(abs(x1 - x0), abs(y1 - y0), 1)
        for i in range(steps + 1):
            self.put(round(x0 + (x1 - x0) * i / steps), round(y0 + (y1 - y0) * i / steps), color)

    def art(self, rows, palette):
        for y, row in enumerate(rows):
            for x, key in enumerate(row):
                if key != '.':
                    self.put(x, y, palette[key])

    def outline(self, color):
        """A one pixel rim around what is drawn, as vanilla's items have."""
        rim = []
        for y in range(self.height):
            for x in range(self.width):
                if self.get(x, y)[3] == 0 and any(
                        self.get(x + dx, y + dy)[3] for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))):
                    rim.append((x, y))
        for x, y in rim:
            self.put(x, y, color)

    def png(self):
        raw = b''.join(b'\x00' + bytes(v for pixel in row for v in pixel) for row in self.pixels)

        def chunk(kind, data):
            body = kind + data
            return struct.pack('>I', len(data)) + body + struct.pack('>I', zlib.crc32(body))

        header = struct.pack('>IIBBBBB', self.width, self.height, 8, 6, 0, 0, 0)
        return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header)
                + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))


OUTLINE = rgb(0x1B1B26)
WOOD, WOOD_DARK = rgb(0x8A6238), rgb(0x5C3F22)

BOTTLE = [
    '................',
    '................',
    '......cccc......',
    '......gwwg......',
    '......g..g......',
    '.....g....g.....',
    '....g.hLLL.g....',
    '...g.hLLLLL.g...',
    '...ghLLLLLLDg...',
    '...gLLLLLLLDg...',
    '...gLLLLLLDDg...',
    '....gLLLLDDg....',
    '.....gggggg.....',
    '................',
    '................',
    '................',
]


def bottle(liquid):
    canvas = Canvas(16, 16)
    liquid = rgb(liquid)
    canvas.art(BOTTLE, {'c': rgb(0x9C6B3A), 'g': rgb(0xC9DDE6, 235), 'w': rgb(0xEAF4F8, 200),
                        'h': shade(liquid, 1.35), 'L': liquid, 'D': shade(liquid, 0.72)})
    return canvas


def weapon(draw):
    canvas = Canvas(16, 16)
    draw(canvas)
    canvas.outline(OUTLINE)
    return canvas


def handle(canvas, x0, y0, x1, y1):
    canvas.line(x0, y0, x1, y1, WOOD)
    canvas.put(x0, y0, WOOD_DARK)


def blade(canvas, x0, y0, x1, y1, metal, width=1):
    """A blade from the guard (x0, y0) up to its tip (x1, y1), lit from the top left."""
    for offset in range(1 - width, width + 1):
        tone = 1.22 if offset == 1 - width else 0.74 if offset == width else 1.0
        canvas.line(x0 + offset, y0, x1 + offset, y1, shade(metal, tone))
    canvas.put(x1, y1, shade(metal, 1.3))


IRON, GOLD = rgb(0xC8CED6), rgb(0xE2B33C)


def greatsword(c):
    handle(c, 1, 14, 4, 11)
    c.line(3, 9, 6, 12, rgb(0x5B6470))
    blade(c, 5, 10, 13, 2, IRON, width=2)


def katana(c):
    handle(c, 2, 14, 4, 12)
    c.put(5, 11, GOLD)
    c.put(4, 11, shade(GOLD, 0.8))
    c.put(5, 12, shade(GOLD, 0.8))
    for x, y in ((6, 10), (7, 9), (8, 8), (9, 7), (10, 6), (11, 4), (11, 5), (12, 3), (13, 2)):
        c.put(x, y, IRON)
    for x, y in ((6, 9), (7, 8), (8, 7), (9, 6), (10, 5)):
        c.put(x, y, shade(IRON, 1.2))


def dagger(c):
    handle(c, 3, 13, 5, 11)
    c.line(4, 9, 7, 12, rgb(0x5B6470))
    blade(c, 6, 9, 10, 5, IRON)
    c.put(7, 9, shade(IRON, 0.74))


def glaive(c):
    handle(c, 1, 14, 9, 6)
    for x, y in ((10, 5), (11, 4), (12, 3), (13, 2), (11, 5), (12, 4), (13, 3), (12, 5), (13, 4)):
        c.put(x, y, IRON)
    for x, y in ((10, 4), (11, 3), (12, 2)):
        c.put(x, y, shade(IRON, 1.2))
    c.put(13, 5, shade(IRON, 0.74))


def warhammer(c):
    handle(c, 2, 14, 9, 7)
    stone = rgb(0x7E8794)
    c.rect(8, 2, 13, 6, stone)
    c.rect(8, 2, 13, 2, shade(stone, 1.25))
    c.rect(8, 6, 13, 6, shade(stone, 0.7))
    c.rect(13, 2, 13, 6, shade(stone, 0.7))
    c.put(10, 4, shade(stone, 0.8))


def rapier(c):
    handle(c, 2, 14, 3, 13)
    for x, y in ((3, 11), (4, 10), (5, 11), (4, 12), (5, 12), (3, 12)):
        c.put(x, y, GOLD)
    c.put(4, 11, shade(GOLD, 1.2))
    c.line(5, 10, 13, 2, IRON)
    c.line(6, 9, 9, 6, shade(IRON, 1.2))


def halberd(c):
    handle(c, 1, 14, 11, 4)
    c.put(12, 3, IRON)
    c.put(13, 2, shade(IRON, 1.2))
    for x, y in ((8, 3), (9, 3), (9, 4), (8, 4), (7, 4), (8, 5), (7, 5), (9, 2), (10, 2)):
        c.put(x, y, IRON)
    for x, y in ((7, 3), (8, 2), (9, 1)):
        c.put(x, y, shade(IRON, 1.2))
    for x, y in ((12, 6), (12, 7), (11, 7)):
        c.put(x, y, shade(IRON, 0.74))


def chain_whip(c):
    handle(c, 2, 14, 4, 12)
    link, dark = rgb(0x9AA3AE), rgb(0x596069)
    path = ((5, 11), (6, 10), (7, 10), (8, 9), (9, 8), (10, 8), (11, 7), (12, 6), (12, 5), (13, 4),
            (13, 3), (12, 2), (11, 2))
    for i, (x, y) in enumerate(path):
        c.put(x, y, link if i % 2 == 0 else dark)
    c.put(10, 2, shade(IRON, 1.2))
    c.put(10, 1, IRON)


CHUNK = [
    '................',
    '................',
    '................',
    '......hh........',
    '.....hmmd..hh...',
    '....hmmmmd.md...',
    '....mmmmdd.dd...',
    '.....mddd.......',
    '..hh......hhh...',
    '.hmmd....hmmmd..',
    '.mmmdd..hmmmmmd.',
    '.mdddd..mmmmmdd.',
    '..dd.....mmdddd.',
    '..........ddd...',
    '................',
    '................',
]


def ore_bits(metal, washed):
    canvas = Canvas(16, 16)
    stone = rgb(0x8C8C8C)
    base = rgb(metal) if washed else stone
    canvas.art(CHUNK, {'h': shade(base, 1.25), 'm': base, 'd': shade(base, 0.7)})
    if not washed:
        for x, y in ((6, 5), (7, 6), (2, 10), (3, 11), (10, 10), (12, 11), (11, 9)):
            canvas.put(x, y, rgb(metal))
    canvas.outline(OUTLINE)
    return canvas


def droplet(fill):
    """A 9x9 HUD droplet: 0 empty, 1 the right half, 2 full (the food row fills from the right)."""
    rows = ['....#....', '...#o#...', '...#o#...', '..#ooo#..', '.#ooooo#.', '.#ooooo#.', '.#ooooo#.',
            '..#ooo#..', '...###...']
    canvas = Canvas(9, 9)
    water, rim, hollow = rgb(0x3F76E4), rgb(0x10182A), rgb(0x2A3350)
    for y, row in enumerate(rows):
        for x, key in enumerate(row):
            if key == '#':
                canvas.put(x, y, rim)
            elif key == 'o':
                wet = fill == 2 or (fill == 1 and x >= 4)
                canvas.put(x, y, water if wet else hollow)
    if fill:
        canvas.put(5, 4, rgb(0x9CC4FF))
    return canvas


def parched_icon():
    canvas = Canvas(18, 18)
    earth, crack = rgb(0xC9A66B), rgb(0x6B4A22)
    canvas.rect(2, 2, 15, 15, earth)
    canvas.rect(2, 2, 15, 2, shade(earth, 1.15))
    for x0, y0, x1, y1 in ((8, 2, 6, 7), (6, 7, 10, 10), (10, 10, 7, 15), (6, 7, 2, 9), (10, 10, 15, 8),
                           (12, 2, 13, 5)):
        canvas.line(x0, y0, x1, y1, crack)
    canvas.outline(OUTLINE)
    return canvas


def anchor_side():
    canvas = Canvas(16, 16)
    stone, band = rgb(0x6E7480), rgb(0x3FA7A0)
    canvas.rect(0, 0, 15, 15, stone)
    for y in range(16):
        for x in range(16):
            if (x * 7 + y * 13) % 11 == 0:
                canvas.put(x, y, shade(stone, 0.86))
            elif (x * 5 + y * 3) % 17 == 0:
                canvas.put(x, y, shade(stone, 1.12))
    canvas.rect(0, 6, 15, 9, shade(band, 0.7))
    canvas.rect(0, 7, 15, 8, band)
    for x in (2, 7, 12):
        canvas.rect(x, 7, x + 1, 8, shade(band, 1.4))
    canvas.rect(0, 0, 15, 0, shade(stone, 1.2))
    canvas.rect(0, 15, 15, 15, shade(stone, 0.7))
    return canvas


def anchor_top():
    canvas = Canvas(16, 16)
    stone, band = rgb(0x6E7480), rgb(0x3FA7A0)
    canvas.rect(0, 0, 15, 15, stone)
    canvas.rect(1, 1, 14, 14, shade(stone, 0.9))
    canvas.rect(4, 4, 11, 11, shade(band, 0.7))
    canvas.rect(5, 5, 10, 10, band)
    canvas.rect(7, 7, 8, 8, shade(band, 1.5))
    for x, y in ((7, 2), (8, 2), (7, 13), (8, 13), (2, 7), (2, 8), (13, 7), (13, 8)):
        canvas.put(x, y, band)
    return canvas


# ---------------------------------------------------------------------------- tables

DRINK = {
    'consume_seconds': 1.6, 'animation': 'drink', 'sound': 'minecraft:entity.generic.drink',
    'has_consume_particles': False,
}

# id: (name, liquid colour, food or None, recipe)
DRINKS = {
    'purified_water': ('Purified Water', 0x4C8DF0, None, None),
    'cactus_water': ('Cactus Water', 0x8FCB7A, None,
                     {'type': 'minecraft:crafting_shapeless', 'category': 'misc',
                      'ingredients': ['minecraft:cactus', 'minecraft:glass_bottle']}),
    'berry_juice': ('Berry Juice', 0xC2365A, {'nutrition': 2, 'saturation': 0.6, 'can_always_eat': True},
                    {'type': 'minecraft:crafting_shapeless', 'category': 'misc',
                     'ingredients': ['minecraft:sweet_berries', 'minecraft:sweet_berries',
                                     'minecraft:sweet_berries', 'minecraft:glass_bottle']}),
    'melon_juice': ('Melon Juice', 0xE8626E, {'nutrition': 2, 'saturation': 0.6, 'can_always_eat': True},
                    {'type': 'minecraft:crafting_shapeless', 'category': 'misc',
                     'ingredients': ['minecraft:melon_slice', 'minecraft:melon_slice',
                                     'minecraft:melon_slice', 'minecraft:glass_bottle']}),
}

KEY = {'I': 'minecraft:iron_ingot', 'S': 'minecraft:stick', 'N': 'minecraft:iron_nugget',
       'G': 'minecraft:gold_ingot', 'L': 'minecraft:leather', 'B': 'minecraft:iron_block',
       'C': 'minecraft:iron_chain', 'O': 'minecraft:obsidian'}

# id: (name, damage, attacks a second, extra reach, durability, drawing, recipe pattern)
# The numbers are what the hand shows; hz_arsenal's movesets are what sets them apart.
WEAPONS = {
    'greatsword': ('Greatsword', 9.0, 0.8, 0.5, 420, greatsword, [' I ', 'III', ' S ']),
    'katana': ('Katana', 6.0, 1.9, 0.0, 300, katana, ['  I', ' I ', 'L  ']),
    'dagger': ('Dagger', 4.0, 3.0, -0.5, 220, dagger, ['I', 'S']),
    'glaive': ('Glaive', 6.5, 1.2, 1.0, 320, glaive, ['  I', ' SI', 'S  ']),
    'warhammer': ('Warhammer', 10.0, 0.6, 0.0, 480, warhammer, ['BIB', ' S ', ' S ']),
    'rapier': ('Rapier', 5.0, 2.4, 0.5, 260, rapier, ['  N', ' I ', 'G  ']),
    'halberd': ('Halberd', 8.0, 0.9, 1.5, 360, halberd, [' II', ' SI', 'S  ']),
    'chain_whip': ('Chain Whip', 4.0, 1.4, 2.5, 240, chain_whip, ['C  ', ' C ', '  S']),
}

# id: (name, the raw item it copies, metal colour, ingot)
METALS = {
    'iron': ('Iron', 'minecraft:raw_iron', 0xD8AF93, 'minecraft:iron_ingot'),
    'copper': ('Copper', 'minecraft:raw_copper', 0xC15A36, 'minecraft:copper_ingot'),
    'gold': ('Gold', 'minecraft:raw_gold', 0xF5CC27, 'minecraft:gold_ingot'),
}

# ---------------------------------------------------------------------------- output


class Pack:
    def __init__(self):
        self.files = {}
        self.lang = {}
        self.tab = []

    def json(self, path, value):
        self.files[path] = (json.dumps(value, indent=2) + '\n').encode()

    def texture(self, path, canvas):
        self.files[f'assets/{MOD}/textures/{path}.png'] = canvas.png()

    def item(self, name, title, declaration, canvas, parent='minecraft:item/generated'):
        self.json(f'data/{MOD}/horizon/item/{name}.json', declaration)
        self.json(f'assets/{MOD}/items/{name}.json',
                  {'model': {'type': 'minecraft:model', 'model': f'{MOD}:item/{name}'}})
        self.json(f'assets/{MOD}/models/item/{name}.json',
                  {'parent': parent, 'textures': {'layer0': f'{MOD}:item/{name}'}})
        self.texture(f'item/{name}', canvas)
        self.lang[f'item.{MOD}.{name}'] = title
        self.tab.append(f'{MOD}:{name}')

    def recipe(self, name, recipe):
        self.json(f'data/{MOD}/recipe/{name}.json', recipe)


def build():
    pack = Pack()

    # Drinks: a bottle that is drunk and leaves its glass, as a honey bottle does.
    for name, (title, liquid, food, recipe) in DRINKS.items():
        components = {'minecraft:consumable': DRINK,
                      'minecraft:use_remainder': {'id': 'minecraft:glass_bottle'}}
        if food:
            components['minecraft:food'] = food
        else:
            components['!minecraft:food'] = {}
        pack.item(name, title, {'template': 'minecraft:honey_bottle', 'max_stack_size': 16,
                                'components': components}, bottle(liquid))
        if recipe:
            pack.recipe(name, {**recipe, 'result': {'id': f'{MOD}:{name}'}})
    # Boiling: a recipe matches an item, not what a bottle holds, so any potion boils
    # down to clean water.
    for kind, seconds in (('smelting', 200), ('campfire_cooking', 300), ('smoking', 100)):
        pack.recipe(f'purified_water_from_{kind}', {
            'type': f'minecraft:{kind}', 'category': 'food', 'ingredient': 'minecraft:potion',
            'result': {'id': f'{MOD}:purified_water'}, 'experience': 0.1, 'cookingtime': seconds})

    # Weapons: an iron sword's item with its own numbers. Attack speed is given as
    # attacks a second; the attribute is an offset from the player's base 4.
    for name, (title, damage, speed, reach, durability, draw, pattern) in WEAPONS.items():
        modifiers = [
            {'type': 'minecraft:attack_damage', 'id': 'minecraft:base_attack_damage',
             'amount': damage - 1.0, 'operation': 'add_value', 'slot': 'mainhand'},
            {'type': 'minecraft:attack_speed', 'id': 'minecraft:base_attack_speed',
             'amount': round(speed - 4.0, 2), 'operation': 'add_value', 'slot': 'mainhand'},
        ]
        if reach:
            modifiers.append({'type': 'minecraft:entity_interaction_range', 'id': f'{MOD}:{name}_reach',
                              'amount': reach, 'operation': 'add_value', 'slot': 'mainhand'})
        pack.item(name, title, {'template': 'minecraft:iron_sword', 'components': {
            'minecraft:max_damage': durability,
            'minecraft:attribute_modifiers': modifiers,
        }}, weapon(draw), parent='minecraft:item/handheld')
        keys = sorted({key for row in pattern for key in row if key != ' '})
        pack.recipe(name, {'type': 'minecraft:crafting_shaped', 'category': 'equipment',
                           'pattern': pattern, 'key': {key: KEY[key] for key in keys},
                           'result': {'id': f'{MOD}:{name}'}})
    weapons = [f'{MOD}:{name}' for name in WEAPONS]
    pack.json(f'data/{MOD}/tags/item/weapons.json', {'values': weapons})
    for tag in ('swords', 'enchantable/weapon', 'enchantable/sharp_weapon', 'enchantable/durability',
                'enchantable/fire_aspect', 'enchantable/sweeping'):
        pack.json(f'data/minecraft/tags/item/{tag}.json', {'values': weapons})

    # Refining: raw ore is crushed (stonecutter), washed (a water cauldron, by
    # hz_survival) and only then smelted whole.
    for metal, (title, raw, colour, ingot) in METALS.items():
        pack.item(f'crushed_{metal}', f'Crushed {title} Ore', {'template': raw},
                  ore_bits(colour, washed=False))
        pack.item(f'washed_{metal}', f'Washed {title} Ore', {'template': raw},
                  ore_bits(colour, washed=True))
        pack.recipe(f'crushed_{metal}', {'type': 'minecraft:stonecutting', 'ingredient': raw,
                                         'result': {'id': f'{MOD}:crushed_{metal}'}})
        for kind, seconds in (('smelting', 200), ('blasting', 100)):
            pack.recipe(f'{metal}_ingot_from_{kind}_washed', {
                'type': f'minecraft:{kind}', 'category': 'misc', 'ingredient': f'{MOD}:washed_{metal}',
                'result': {'id': ingot}, 'experience': 0.7, 'cookingtime': seconds})
    pack.json(f'data/{MOD}/tags/item/crushed_ores.json',
              {'values': [f'{MOD}:crushed_{metal}' for metal in METALS]})

    # The climate anchor: Biome Shift leaves the land around it as it is.
    pack.json(f'data/{MOD}/horizon/block/climate_anchor.json', {
        'template': 'minecraft:lodestone',
        'properties': {'hardness': 3.5, 'light': 7, 'sounds': 'minecraft:lodestone'},
        'item': True})
    pack.json(f'assets/{MOD}/blockstates/climate_anchor.json',
              {'variants': {'': {'model': f'{MOD}:block/climate_anchor'}}})
    pack.json(f'assets/{MOD}/models/block/climate_anchor.json', {
        'parent': 'minecraft:block/cube_column',
        'textures': {'end': f'{MOD}:block/climate_anchor_top', 'side': f'{MOD}:block/climate_anchor_side'}})
    pack.json(f'assets/{MOD}/items/climate_anchor.json',
              {'model': {'type': 'minecraft:model', 'model': f'{MOD}:block/climate_anchor'}})
    pack.texture('block/climate_anchor_side', anchor_side())
    pack.texture('block/climate_anchor_top', anchor_top())
    pack.json(f'data/{MOD}/loot_table/blocks/climate_anchor.json', {
        'type': 'minecraft:block',
        'pools': [{'rolls': 1, 'entries': [{'type': 'minecraft:item', 'name': f'{MOD}:climate_anchor'}],
                   'conditions': [{'condition': 'minecraft:survives_explosion'}]}]})
    pack.json('data/minecraft/tags/block/mineable/pickaxe.json', {'values': [f'{MOD}:climate_anchor']})
    pack.recipe('climate_anchor', {
        'type': 'minecraft:crafting_shaped', 'category': 'misc',
        'pattern': ['ACA', 'CLC', 'ACA'],
        'key': {'A': 'minecraft:amethyst_shard', 'C': 'minecraft:copper_ingot', 'L': 'minecraft:lodestone'},
        'result': {'id': f'{MOD}:climate_anchor'}})
    pack.lang[f'block.{MOD}.climate_anchor'] = 'Climate Anchor'
    pack.tab.append(f'{MOD}:climate_anchor')

    # Thirst: the effect that shows it (hz_survival gives it) and the HUD's droplets.
    pack.json(f'data/{MOD}/horizon/mob_effect/parched.json', {'category': 'harmful', 'color': 0xC9A66B})
    pack.texture('mob_effect/parched', parched_icon())
    pack.lang[f'effect.{MOD}.parched'] = 'Parched'
    for name, fill in (('thirst_empty', 0), ('thirst_half', 1), ('thirst_full', 2)):
        pack.texture(f'gui/{name}', droplet(fill))

    pack.json(f'data/{MOD}/horizon/creative_tab/hardship.json',
              {'title': 'Horizon Hardship', 'icon': f'{MOD}:greatsword', 'items': pack.tab})
    pack.json(f'assets/{MOD}/lang/en_us.json', dict(sorted(pack.lang.items())))
    return pack.files


def main():
    files = build()
    for folder in ('assets', 'data'):
        shutil.rmtree(PACK / folder, ignore_errors=True)
    for path, content in files.items():
        target = PACK / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)
    print(f'wrote {len(files)} files to {PACK}')


if __name__ == '__main__':
    main()
