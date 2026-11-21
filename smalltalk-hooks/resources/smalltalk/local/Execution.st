Execution extend [
        whileTrue: aBlock [
                <primitive: VMpr_Block_whileTrue>
                self value ifTrue: [aBlock value. self whileTrue: aBlock]                
        ]  
]
