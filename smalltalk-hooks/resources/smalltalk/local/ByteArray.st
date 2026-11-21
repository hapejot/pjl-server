ArrayedCollection subclass: ByteArray [
    <shape: #byte>

    ByteArray class >> new: size [
        ^self basicNew: size
    ]
]
