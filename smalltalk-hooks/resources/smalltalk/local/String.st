ArrayedCollection subclass: String [
    <shape: #ushort>

    String class >> new: size [
        ^self basicNew: size
    ]

    isString [
        ^true
    ]
]
