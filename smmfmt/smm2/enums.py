"""SMM2 enumerations. Names are our own; numbers are the values stored in course data."""

from enum import IntEnum


class GameStyle:
    """Course header stores the style as a two-character ASCII code."""

    SMB1 = "M1"
    SMB3 = "M3"
    SMW = "MW"
    NSMBU = "WU"
    SM3DW = "3W"
    ALL = (SMB1, SMB3, SMW, NSMBU, SM3DW)


class Theme(IntEnum):
    GROUND = 0
    UNDERGROUND = 1
    CASTLE = 2
    AIRSHIP = 3
    UNDERWATER = 4
    GHOST_HOUSE = 5
    SNOW = 6
    DESERT = 7
    SKY = 8
    FOREST = 9


class AutoscrollType(IntEnum):
    NONE = 0
    SLOW = 1
    NORMAL = 2
    FAST = 3
    CUSTOM = 4


class ClearConditionCategory(IntEnum):
    NONE = 0
    PARTS = 1
    STATUS = 2
    ACTIONS = 3


class ObjectId(IntEnum):
    GOOMBA = 0
    KOOPA = 1
    PIRANHA_PLANT = 2
    HAMMER_BRO = 3
    BRICK = 4
    QUESTION_BLOCK = 5
    HARD_BLOCK = 6
    GROUND = 7
    COIN = 8
    PIPE = 9
    SPRING = 10
    LIFT = 11
    THWOMP = 12
    BILL_BLASTER = 13
    MUSHROOM_PLATFORM = 14
    BOB_OMB = 15
    SEMISOLID = 16
    BRIDGE = 17
    P_SWITCH = 18
    POW = 19
    SUPER_MUSHROOM = 20
    DONUT_BLOCK = 21
    CLOUD = 22
    NOTE_BLOCK = 23
    FIRE_BAR = 24
    SPINY = 25
    GOAL_GROUND = 26
    GOAL = 27
    BUZZY_BEETLE = 28
    HIDDEN_BLOCK = 29
    LAKITU = 30
    LAKITU_CLOUD = 31
    BANZAI_BILL = 32
    ONE_UP = 33
    FIRE_FLOWER = 34
    SUPER_STAR = 35
    LAVA_LIFT = 36
    START_GROUND = 37
    START_SIGN = 38
    MAGIKOOPA = 39
    SPIKE_TOP = 40
    BOO = 41
    KOOPA_CLOWN_CAR = 42
    SPIKE_TRAP = 43
    STYLE_POWERUP = 44
    SHOE_GOOMBA = 45
    DRY_BONES = 46
    CANNON = 47
    BLOOPER = 48
    CASTLE_BRIDGE = 49
    HOP_CHOPS = 50
    SKIPSQUEAK = 51
    WIGGLER = 52
    CONVEYOR_BELT = 53
    BURNER = 54
    DOOR = 55
    CHEEP_CHEEP = 56
    MUNCHER = 57
    ROCKY_WRENCH = 58
    TRACK = 59
    LAVA_BUBBLE = 60
    CHAIN_CHOMP = 61
    BOWSER = 62
    ICE_BLOCK = 63
    VINE = 64
    STINGBY = 65
    ARROW = 66
    ONE_WAY_WALL = 67
    GRINDER = 68
    PLAYER = 69
    BIG_COIN = 70
    HALF_COLLISION_PLATFORM = 71
    KOOPA_CAR = 72
    TOAD = 73
    SPIKE = 74
    STONE = 75
    TWISTER = 76
    BOOM_BOOM = 77
    POKEY = 78
    P_BLOCK = 79
    DASH_BLOCK = 80
    STYLE_POWERUP_2 = 81
    BUMPER = 82
    SKEWER = 83
    SNAKE_BLOCK = 84
    TRACK_BLOCK = 85
    CHARVAARGH = 86
    GENTLE_SLOPE = 87
    STEEP_SLOPE = 88
    AUTOSCROLL_CAMERA = 89
    CHECKPOINT_FLAG = 90
    SEESAW = 91
    PINK_COIN = 92
    CLEAR_PIPE = 93
    CONVEYOR_SLOPE = 94
    KEY = 95
    ANT_TROOPER = 96
    WARP_BOX = 97
    BOWSER_JR = 98
    ON_OFF_SWITCH = 99
    DOTTED_LINE_BLOCK = 100
    WATER_MARKER = 101
    MONTY_MOLE = 102
    FISH_BONE = 103
    ANGRY_SUN = 104
    SWINGING_CLAW = 105
    TREE = 106
    PIRANHA_CREEPER = 107
    BLINKING_BLOCK = 108
    SOUND_EFFECT = 109
    SPIKE_BLOCK = 110
    MECHAKOOPA = 111
    CRATE = 112
    MUSHROOM_TRAMPOLINE = 113
    PORCUPUFFER = 114
    TOADETTE = 115
    SUPER_HAMMER = 116
    BULLY = 117
    ICICLE = 118
    EXCLAMATION_BLOCK = 119
    LEMMY = 120
    MORTON = 121
    LARRY = 122
    WENDY = 123
    IGGY = 124
    ROY = 125
    LUDWIG = 126
    CANNON_BOX = 127
    PROPELLER_BOX = 128
    GOOMBA_MASK = 129
    BULLET_BILL_MASK = 130
    RED_POW_BOX = 131
    ON_OFF_TRAMPOLINE = 132
